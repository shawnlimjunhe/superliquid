use std::collections::{HashMap, HashSet};

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use tokio::{
    pin,
    sync::mpsc::{self, error::SendError},
    time::sleep,
};

use crate::{config, node::ReplicaOutbound};

use super::{
    block::{Block, BlockHash},
    client_command::ClientCommand,
    crypto::{PartialSig, QuorumCertificate},
    message::{HotStuffMessage, HotStuffMessageType},
    pacemaker::{self, Pacemaker},
};

pub type ViewNumber = u64;

type MessageKey = (BlockHash, ViewNumber, HotStuffMessageType);

pub struct HotStuffReplica {
    node_id: usize,
    pub validator_set: HashSet<VerifyingKey>,
    signing_key: SigningKey,
    view_number: ViewNumber,
    locked_qc: Option<QuorumCertificate>,
    prepare_qc: Option<QuorumCertificate>,
    precommit_qc: Option<QuorumCertificate>,
    commit_qc: Option<QuorumCertificate>,
    current_proposal: Option<Block>,
    pub messages: Vec<HotStuffMessage>,
    blockstore: HashMap<BlockHash, Block>,

    pub v_height: u128,
    pub locked_node: Option<Block>,
    pub last_exec_node: Option<Block>,
    // genesis: Option<Block>,
    pub pacemaker: Pacemaker,
    node_sender: mpsc::Sender<ReplicaOutbound>,
}

impl HotStuffReplica {
    pub fn new(node_id: usize, node_sender: mpsc::Sender<ReplicaOutbound>) -> Self {
        let signing_key = config::retrieve_signing_key(node_id);

        HotStuffReplica {
            node_id,
            validator_set: config::retrieve_validator_set(),
            signing_key,
            view_number: 0,
            locked_qc: None,
            prepare_qc: None,
            precommit_qc: None,
            commit_qc: None,
            current_proposal: None,
            messages: vec![],
            blockstore: HashMap::new(),
            pacemaker: Pacemaker::new(),
            node_sender,

            v_height: 0,
            locked_node: None,
            last_exec_node: None,
            // genesis: None,
        }
    }

    pub fn get_public_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    fn sign(&self, message: &HotStuffMessage) -> PartialSig {
        let message_hash = message.hash();
        let signature = self.signing_key.sign(&message_hash);

        PartialSig {
            signer_id: self.get_public_key(),
            signature,
        }
    }

    pub fn process_message(&mut self, message: HotStuffMessage) {
        self.messages.push(message)
    }

    pub fn vote_message(
        &self,
        message_type: HotStuffMessageType,
        node: Block,
        qc: QuorumCertificate,
        curr_view: ViewNumber,
    ) -> HotStuffMessage {
        let mut message = HotStuffMessage::new(message_type, Some(node), Some(qc), curr_view);
        message.partial_sig = Some(self.sign(&message));
        message
    }

    pub fn matching_message(
        message: HotStuffMessage,
        message_type: HotStuffMessageType,
        view_number: ViewNumber,
    ) -> bool {
        message_type == message.message_type && view_number == message.view_number
    }

    pub fn matching_qc(
        qc: &QuorumCertificate,
        message_type: HotStuffMessageType,
        view_number: ViewNumber,
    ) -> bool {
        qc.message_type == message_type && qc.view_number == view_number
    }

    fn quorum_threshold(&self) -> usize {
        let n = self.validator_set.len();
        2 * ((n - 1) / 3) + 1
    }

    pub fn get_highest_vote_group<'a>(votes: Vec<&'a HotStuffMessage>) -> Vec<&'a HotStuffMessage> {
        if votes.len() < 2 {
            return votes; // returns Vec<&HotStuffMessage>
        }

        let mut groups: HashMap<MessageKey, Vec<&'a HotStuffMessage>> = HashMap::new();
        let mut max_key: Option<MessageKey> = None;
        let mut max_size: usize = 0;

        // aggregates votes by message_key
        for vote in votes {
            let block_hash = match &vote.node {
                Some(block) => block.hash(),
                None => [0; 32],
            };

            let message_key = (block_hash, vote.view_number, vote.message_type.clone());
            let cloned_message_key = message_key.clone();
            let group = groups.entry(message_key).or_default();
            group.push(vote);

            if group.len() > max_size {
                max_key = Some(cloned_message_key);
                max_size = group.len();
            }
        }

        let key = match max_key {
            Some(message_key) => message_key,
            None => {
                return vec![];
            }
        };

        // return group with the highest count
        // in this case, we dont really care if there are mulitple groups with the same count
        groups.get(&key).cloned().unwrap_or_default()
    }

    fn validate_vote_signatures<'a>(
        &self,
        votes: &'a Vec<HotStuffMessage>,
    ) -> Vec<&'a HotStuffMessage> {
        let mut validated_votes = vec![];

        let validator_set = &self.validator_set;
        let mut seen_validators = HashSet::new();

        for vote in votes.iter() {
            if let Some(partial_sig) = &vote.partial_sig {
                let verifying_key = &partial_sig.signer_id;

                if !validator_set.contains(verifying_key) || !seen_validators.insert(verifying_key)
                {
                    // reject if not part of validator set or validator already voted
                    continue;
                }

                if verifying_key
                    .verify_strict(&vote.hash(), &partial_sig.signature)
                    .is_ok()
                {
                    validated_votes.push(vote);
                }
            }
        }

        validated_votes
    }

    pub fn validate_votes(&self, votes: &Vec<HotStuffMessage>) -> bool {
        // validate votes:
        // - all from known validators
        // - same block hash, view, type
        // - signatures are valid
        // - valid votes above quorum

        if votes.len() > self.validator_set.len() {
            return false;
        }

        let quorum_threhold = self.quorum_threshold();

        // Validate votes before grouping to defend against spam
        let validated_votes = self.validate_vote_signatures(&votes);

        if validated_votes.len() < quorum_threhold {
            return false;
        }

        let votes = Self::get_highest_vote_group(validated_votes);

        votes.len() >= quorum_threhold
    }

    pub fn create_qc_from_votes(&self, votes: &Vec<HotStuffMessage>) -> Option<QuorumCertificate> {
        if self.validate_votes(votes) {
            QuorumCertificate::from_votes_unchecked(votes)
        } else {
            None
        }
    }

    pub fn safe_node(&self, block: &Block, qc: QuorumCertificate) -> bool {
        match &self.locked_qc {
            Some(locked_qc) => {
                let locked_block_hash = locked_qc.block_hash;
                let extends = block.extends_from(locked_block_hash, &self.blockstore);
                let newer_qc = qc.view_number > locked_qc.view_number;

                extends || newer_qc
            }
            None => true,
        }
    }

    pub fn get_highest_qc_from_votes<'a>(
        &self,
        votes: &'a Vec<HotStuffMessage>,
    ) -> Option<&'a QuorumCertificate> {
        votes
            .iter()
            .filter_map(|msg| match msg.message_type {
                HotStuffMessageType::NewView => {
                    if msg.view_number == self.view_number - 1 {
                        return msg.justify.as_ref();
                    }
                    None
                }
                _ => None,
            })
            .max_by_key(|qc| qc.view_number)
    }

    // hot stuff phases
    pub fn leader_prepare(&mut self, cmd: &ClientCommand) -> Option<HotStuffMessage> {
        let votes = &self.messages;
        let Some(high_qc) = self.get_highest_qc_from_votes(votes) else {
            // no valid QC from previous view
            return None;
        };

        let Some(parent) = self.blockstore.get(&high_qc.block_hash) else {
            // cant find QC's block
            return None;
        };

        let new_block = Block::create_leaf(parent, cmd.clone(), self.view_number);
        self.blockstore.insert(new_block.hash(), new_block.clone());

        return Some(HotStuffMessage::new(
            HotStuffMessageType::Prepare,
            Some(new_block),
            Some(high_qc.clone()),
            self.view_number,
        ));
    }

    pub fn replica_prepare(&self, msg: HotStuffMessage) -> Option<HotStuffMessage> {
        let Some(msg_justify_qc) = msg.justify else {
            return None;
        };

        let Some(block) = &msg.node else {
            return None;
        };

        if !block.extends_from(msg_justify_qc.block_hash, &self.blockstore) {
            return None;
        }

        if self.safe_node(block, msg_justify_qc) {
            return Some(HotStuffMessage::new(
                HotStuffMessageType::Prepare,
                msg.node.clone(),
                None,
                self.view_number,
            ));
        }
        None
    }

    pub fn leader_precommit(&mut self) -> Option<HotStuffMessage> {
        let votes = &self.messages;
        let Some(qc) = self.create_qc_from_votes(votes) else {
            // unable to form a QC
            return None;
        };

        self.prepare_qc = Some(qc.clone());
        return Some(HotStuffMessage::new(
            HotStuffMessageType::PreCommit,
            None,
            Some(qc),
            self.view_number,
        ));
    }

    pub fn replica_precommit(&mut self, msg: &HotStuffMessage) -> Option<HotStuffMessage> {
        let Some(qc) = msg.justify.clone() else {
            // no qc to validate
            return None;
        };

        if qc.verify(&self.validator_set, self.quorum_threshold()) {
            self.prepare_qc = Some(qc.clone());
            let Some(node) = self.blockstore.get(&qc.block_hash) else {
                return None;
            };
            return Some(HotStuffMessage::new(
                HotStuffMessageType::PreCommit,
                Some(node.clone()),
                None,
                self.view_number,
            ));
        }
        None
    }
    pub fn leader_commit(&mut self) -> Option<HotStuffMessage> {
        let votes = &self.messages;
        let Some(qc) = self.create_qc_from_votes(votes) else {
            // unable to form a QC
            return None;
        };

        self.precommit_qc = Some(qc.clone());
        return Some(HotStuffMessage::new(
            HotStuffMessageType::Commit,
            None,
            Some(qc),
            self.view_number,
        ));
    }

    pub fn replica_commit(&mut self, msg: &HotStuffMessage) -> Option<HotStuffMessage> {
        let Some(qc) = msg.justify.clone() else {
            // no qc to validate
            return None;
        };

        if qc.verify(&self.validator_set, self.quorum_threshold()) {
            self.locked_qc = Some(qc.clone());
            let Some(node) = self.blockstore.get(&qc.block_hash) else {
                return None;
            };
            return Some(HotStuffMessage::new(
                HotStuffMessageType::Commit,
                Some(node.clone()),
                None,
                self.view_number,
            ));
        }
        None
    }

    pub fn leader_decide(&mut self) -> Option<HotStuffMessage> {
        let votes = &self.messages;
        let Some(qc) = self.create_qc_from_votes(votes) else {
            // unable to form a QC
            return None;
        };

        self.commit_qc = Some(qc.clone());

        // do commit here
        return Some(HotStuffMessage::new(
            HotStuffMessageType::Decide,
            None,
            Some(qc),
            self.view_number,
        ));
    }

    pub fn replica_decide(&mut self, msg: &HotStuffMessage) -> Option<HotStuffMessage> {
        let Some(qc) = msg.justify.clone() else {
            // no qc to validate
            return None;
        };

        if qc.verify(&self.validator_set, self.quorum_threshold())
            && Self::matching_qc(&qc, HotStuffMessageType::Commit, self.view_number)
        {
            todo!()
        }
        None
    }

    pub fn advance_and_create_new_view(&mut self) -> HotStuffMessage {
        self.pacemaker.advance_view();
        self.view_number = self.pacemaker.curr_view;
        HotStuffMessage {
            message_type: HotStuffMessageType::NewView,
            view_number: self.view_number,
            node: None,
            justify: self.prepare_qc.clone(),
            partial_sig: None,
        }
    }

    async fn handle_message<FLeader, FReplica>(
        &mut self,
        msg: HotStuffMessage,
        leader_handler: FLeader,
        replica_handler: FReplica,
    ) -> Result<(), SendError<ReplicaOutbound>>
    where
        FLeader: FnOnce(&mut Self, &HotStuffMessage) -> Option<HotStuffMessage>,
        FReplica: FnOnce(&mut Self, &HotStuffMessage) -> Option<HotStuffMessage>,
    {
        let leader = self.pacemaker.current_leader();
        let is_leader = self.node_id == leader;
        let cloned_msg = msg.clone();
        self.messages.push(msg);

        let outbound_msg = if is_leader {
            leader_handler(self, &cloned_msg)
        } else {
            replica_handler(self, &cloned_msg)
        };

        let Some(outbound_msg) = outbound_msg else {
            return Ok(());
        };

        if is_leader {
            self.node_sender
                .send(ReplicaOutbound::Broadcast(outbound_msg))
                .await?
        } else {
            let leader = self.pacemaker.current_leader();
            self.node_sender
                .send(ReplicaOutbound::SendTo(leader, outbound_msg))
                .await?
        }
        Ok(())
    }

    async fn handle_new_view(
        &mut self,
        msg: HotStuffMessage,
    ) -> Result<(), SendError<ReplicaOutbound>> {
        self.handle_message(
            msg,
            |s, m| {
                if let Some(Block::Normal { cmd, .. }) = &m.node {
                    s.leader_prepare(cmd)
                } else {
                    None
                }
            },
            |s, m| s.replica_prepare(m.clone()),
        )
        .await
    }

    async fn handle_prepare(
        &mut self,
        msg: HotStuffMessage,
    ) -> Result<(), SendError<ReplicaOutbound>> {
        self.handle_message(
            msg,
            |s, _| s.leader_precommit(),
            |s, m| s.replica_precommit(&m),
        )
        .await
    }

    async fn handle_precommit(
        &mut self,
        msg: HotStuffMessage,
    ) -> Result<(), SendError<ReplicaOutbound>> {
        self.handle_message(msg, |s, _| s.leader_commit(), |s, m| s.replica_commit(&m))
            .await
    }

    async fn handle_commit(
        &mut self,
        msg: HotStuffMessage,
    ) -> Result<(), SendError<ReplicaOutbound>> {
        self.handle_message(msg, |s, _| s.leader_decide(), |s, m| s.replica_decide(&m))
            .await
    }

    pub async fn run_replica(
        &mut self,
        mut to_replica_rx: mpsc::Receiver<HotStuffMessage>,
    ) -> Result<(), SendError<ReplicaOutbound>> {
        loop {
            // Refresh pacemaker timer dynamically each loop
            let time_remaining = self.pacemaker.time_remaining();
            let pacemaker_timer = sleep(time_remaining);
            pin!(pacemaker_timer);

            tokio::select! {
                Some(msg) = to_replica_rx.recv() => {
                    match msg.message_type {
                        HotStuffMessageType::NewView => self.handle_new_view(msg).await?,
                        HotStuffMessageType::Prepare => self.handle_prepare(msg).await?,
                        HotStuffMessageType::PreCommit => self.handle_precommit(msg).await?,
                        HotStuffMessageType::Commit => self.handle_commit(msg).await?,
                        HotStuffMessageType::Decide => { /* handle or ignore */ },
                    }
                }

                _ = &mut pacemaker_timer => {
                    if self.pacemaker.should_advance_view() {

                        let outbound_msg = self.advance_and_create_new_view();

                        let leader = self.pacemaker.current_leader();
                        self.node_sender
                            .send(ReplicaOutbound::SendTo(leader, outbound_msg))
                            .await?;
                    }
                }
            }
        }
    }
}

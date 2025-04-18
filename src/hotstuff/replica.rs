use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use futures::future::pending;
use tokio::{
    pin,
    sync::mpsc::{self, error::SendError},
    time::sleep,
};

use crate::{
    config,
    hotstuff::utils,
    replica_debug, replica_log,
    types::{ReplicaInBound, ReplicaOutbound, Transaction},
};

use super::{
    block::{Block, BlockHash},
    client_command::{Action, ClientCommand},
    crypto::{PartialSig, QuorumCertificate},
    message::HotStuffMessage,
    message_window::MessageWindow,
    pacemaker::Pacemaker,
};

pub type ViewNumber = u64;

type MessageKey = (BlockHash, ViewNumber);

pub struct HotStuffReplica {
    pub node_id: usize,
    pub validator_set: HashSet<VerifyingKey>,
    signing_key: SigningKey,

    generic_qc: Arc<QuorumCertificate>,
    locked_qc: Arc<QuorumCertificate>,

    current_proposal: Option<Block>,
    blockstore: HashMap<BlockHash, Block>,
    mempool: VecDeque<Transaction>,

    pub messages: MessageWindow,
    pub local_queue: VecDeque<HotStuffMessage>,
    pub pacemaker: Pacemaker,

    node_sender: mpsc::Sender<ReplicaOutbound>,
}

impl HotStuffReplica {
    pub fn new(node_id: usize, node_sender: mpsc::Sender<ReplicaOutbound>) -> Self {
        let signing_key = config::retrieve_signing_key(node_id);

        let (genesis_block, genesis_qc) = Block::create_genesis_block();
        let mut blockstore: HashMap<BlockHash, Block> = HashMap::new();
        blockstore.insert(genesis_block.hash(), genesis_block.clone());

        let genesis_qc = Arc::new(genesis_qc);
        HotStuffReplica {
            node_id,
            validator_set: config::retrieve_validator_set(),
            signing_key,

            generic_qc: genesis_qc.clone(),
            locked_qc: genesis_qc,

            current_proposal: None,
            blockstore,
            mempool: VecDeque::new(),

            messages: MessageWindow::new(0),
            local_queue: VecDeque::new(),

            pacemaker: Pacemaker::new(),
            node_sender,
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

    pub fn vote_message(
        &self,
        node: &Block,
        option_justify: Option<QuorumCertificate>,
        curr_view: ViewNumber,
    ) -> HotStuffMessage {
        let mut message =
            HotStuffMessage::new(Some(node.clone()), option_justify, curr_view, self.node_id);
        message.partial_sig = Some(self.sign(&message));
        message
    }

    pub fn matching_message(message: HotStuffMessage, view_number: ViewNumber) -> bool {
        view_number == message.view_number
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

            let message_key = (block_hash, vote.view_number);
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
        // Note: this creates an owned Vec<HotStuffMessage>
        let filtered_votes: Vec<HotStuffMessage> = votes.iter().cloned().collect();

        if self.validate_votes(&filtered_votes) {
            QuorumCertificate::from_votes_unchecked(&filtered_votes)
        } else {
            None
        }
    }

    pub fn safe_node(&self, block: &Block, qc: &QuorumCertificate) -> bool {
        let locked_qc = &self.locked_qc.clone();
        let locked_block_hash = locked_qc.block_hash;
        let extends = block.extends_from(locked_block_hash, &self.blockstore);
        let newer_qc = qc.view_number > locked_qc.view_number;

        extends || newer_qc
    }

    fn create_cmd(&mut self) -> ClientCommand {
        let transactions: Vec<Transaction> = self.mempool.iter().cloned().take(1).collect();

        if transactions.len() > 0 {
            let txn = &transactions[0];
            return ClientCommand {
                transactions: Action::Transfer {
                    from: txn.from.clone(),
                    to: txn.to.clone(),
                    amount: txn.amount,
                },
            };
        }
        return ClientCommand {
            transactions: Action::Empty,
        };
    }

    fn leader_create_and_sign_message(&mut self, new_block: Block) -> HotStuffMessage {
        let mut outbound_msg = HotStuffMessage::new(
            Some(new_block),
            None,
            self.pacemaker.curr_view,
            self.node_id,
        );
        let partial_sig = self.sign(&outbound_msg);
        outbound_msg.partial_sig = Some(partial_sig);
        return outbound_msg;
    }

    pub fn leader_handle_message(&mut self) -> Option<HotStuffMessage> {
        let curr_view = self.pacemaker.curr_view;
        replica_log!(
            self.node_id,
            "Leader handle message at view: {:?}",
            curr_view
        );
        let cmd: &ClientCommand = &self.create_cmd();

        self.pacemaker.reset_timer();

        if !utils::has_quorum_for_view(
            self.messages.get_messages_for_view(curr_view),
            self.pacemaker.curr_view,
            self.quorum_threshold(),
        ) {
            replica_debug!(
                self.node_id,
                "waiting for quorum (n - f): view {:?}",
                self.pacemaker.curr_view
            );
            return None;
        }

        let votes = self.messages.get_messages_for_view(curr_view);

        let Some(votes) = votes else {
            replica_debug!(self.node_id, "Failed to get messages for current vote");
            return None;
        };

        let qc = self.create_qc_from_votes(votes);

        match qc {
            Some(qc) => self.generic_qc = Arc::new(qc),
            None => {}
        }

        replica_log!(
            self.node_id,
            "Leader handle new view (Prepare), view num: {:?}",
            self.pacemaker.curr_view
        );

        // msgs should only contain justify if next-view interupt is triggered
        match utils::get_highest_qc_from_votes(&self.messages) {
            Some(high_qc) => {
                if high_qc.view_number > self.generic_qc.view_number {
                    // update generic qc if replica falls behind
                    self.generic_qc = Arc::new(high_qc.clone());
                    replica_debug!(self.node_id, "generic qc replaced");
                }
            }
            None => {}
        };

        let Some(parent) = self.blockstore.get(&self.generic_qc.block_hash) else {
            // cant find QC's block
            replica_debug!(self.node_id, "cant find qc's block: L");
            return None;
        };

        let curr_view = self.pacemaker.curr_view;
        let new_block =
            Block::create_leaf(parent, cmd.clone(), curr_view, (*self.generic_qc).clone());
        self.blockstore.insert(new_block.hash(), new_block.clone());

        self.current_proposal = Some(new_block.clone());
        replica_debug!(self.node_id, "Success: Leader:: Prepare");

        // leader should also vote for their own message
        return Some(self.leader_create_and_sign_message(new_block));
    }

    fn get_justified_block(&self, block: &Block) -> Option<&Block> {
        let hash = match block {
            Block::Normal { justify, .. } => &justify.block_hash,
            Block::Genesis { .. } => return None,
        };
        self.blockstore.get(hash)
    }

    /// Return owned clones, not references:
    fn get_justifed_block_and_qc_cloned(
        &self,
        block: &Block,
    ) -> (Option<Block>, Option<QuorumCertificate>) {
        let justify = match block {
            Block::Normal { justify, .. } => justify,
            Block::Genesis { .. } => return (None, None),
        };

        // Clone the child block from the store so we get an owned `Block`
        let child_block = self.blockstore.get(&justify.block_hash).cloned();

        (child_block, Some(justify.clone()))
    }

    fn is_parent(&self, block_child: &Block, block_parent: &Block) -> bool {
        match block_child {
            Block::Genesis { .. } => false,
            Block::Normal { parent_id, .. } => block_parent.hash() == *parent_id,
        }
    }

    pub fn replica_handle_message(&mut self, msg: HotStuffMessage) -> Option<HotStuffMessage> {
        replica_log!(
            self.node_id,
            "Replica handle message at view: {:?}",
            self.pacemaker.curr_view
        );

        if msg.sender != self.pacemaker.get_leader_for_view(msg.view_number) {
            replica_debug!(
                self.node_id,
                "msg sender: {:?} is not the leader for the current view: {:?}",
                msg.sender,
                msg.view_number
            );
            return None;
        }

        // b*
        let Some(b_star) = msg.node.clone() else {
            replica_debug!(self.node_id, "No node in message");
            return None;
        };
        self.blockstore.insert(b_star.hash(), b_star.clone());

        // b″ := b*.justify.node
        let (Some(b_double_prime), Some(b_star_justify)) =
            self.get_justifed_block_and_qc_cloned(&b_star)
        else {
            replica_debug!(
                self.node_id,
                "Missing b″ justified by b* or missing justify on b*"
            );
            return None;
        };

        let curr_view = self.pacemaker.curr_view;

        let mut outbound_msg = None;

        let is_safe = {
            // This scope ends after the function call
            self.safe_node(&b_star, &b_star_justify)
        };

        let is_valid_sig = { b_star_justify.verify(&self.validator_set, self.quorum_threshold()) };

        if is_safe && is_valid_sig {
            outbound_msg = Some(self.vote_message(&b_star, None, curr_view + 1))
        } else {
            replica_debug!(
                self.node_id,
                "is_safe: {:?}, is_valid_sig: {:?}",
                is_safe,
                is_valid_sig
            );
            return outbound_msg;
        }

        replica_debug!(
            self.node_id,
            "Start pre-commit phase on b*'s parent: view {:?}",
            curr_view
        );

        if !self.is_parent(&b_star, &b_double_prime) {
            return outbound_msg;
        }
        self.generic_qc = Arc::new(b_star_justify);

        replica_debug!(
            self.node_id,
            "Start commit phase on b*'s grandparent: view {:?}",
            curr_view
        );

        // b′ := b″.justify.node
        let (Some(b_prime), Some(b_double_prime_justify)) =
            self.get_justifed_block_and_qc_cloned(&b_double_prime)
        else {
            replica_debug!(self.node_id, "Missing b′ justified by b″");
            return outbound_msg;
        };

        if !self.is_parent(&b_double_prime, &b_prime) {
            return outbound_msg;
        }
        self.locked_qc = Arc::new(b_double_prime_justify);

        self.pacemaker
            .set_last_committed_view(self.locked_qc.clone());

        // b := b′.justify.node
        let Some(b) = self.get_justified_block(&b_prime) else {
            replica_debug!(self.node_id, "Missing b justified by b′");
            return outbound_msg;
        };

        if !self.is_parent(&b_prime, b) {
            return outbound_msg;
        }

        replica_log!(self.node_id, "Commit success on view: {:?}", curr_view);
        // do commit here

        return outbound_msg;
    }

    async fn handle_message(
        &mut self,
        msg: HotStuffMessage,
    ) -> Result<(), SendError<ReplicaOutbound>> {
        // fastforward pacemaker if we are lagging
        self.sync_view(&msg);

        if msg.view_number + 1 < self.pacemaker.curr_view {
            replica_debug!(
                self.node_id,
                "Recieved stale message from view: {:?} at curr_view {:?} from node: {:?}",
                msg.view_number,
                self.pacemaker.curr_view,
                msg.sender
            );
        }

        self.messages.push(msg.clone());

        let leader = self.pacemaker.current_leader();
        let is_leader = self.node_id == leader;
        let curr_view = self.pacemaker.curr_view;

        let outbound_msg = if is_leader {
            self.leader_handle_message()
        } else {
            self.replica_handle_message(msg)
        };

        let Some(outbound_msg) = outbound_msg else {
            return Ok(());
        };

        let next_leader = self.pacemaker.get_leader_for_view(curr_view + 1);

        if is_leader {
            self.node_sender
                .send(ReplicaOutbound::Broadcast(outbound_msg.clone()))
                .await?;
        } else {
            replica_log!(
                self.node_id,
                "Sending msg from: {:?} to: {:?}",
                self.node_id,
                next_leader
            );

            if self.node_id == next_leader {
                self.local_queue.push_back(outbound_msg);
                return Ok(());
            }

            self.node_sender
                .send(ReplicaOutbound::SendTo(next_leader, outbound_msg))
                .await?;
        }
        Ok(())
    }

    fn sync_view(&mut self, msg: &HotStuffMessage) {
        let incoming_view = msg.view_number;
        self.pacemaker.fast_forward_view(incoming_view);
    }

    pub async fn run_replica(
        &mut self,
        mut to_replica_rx: mpsc::Receiver<ReplicaInBound>,
    ) -> Result<(), SendError<ReplicaOutbound>> {
        replica_log!(self.node_id, "Running replica...");
        loop {
            // Refresh pacemaker timer dynamically each loop
            let time_remaining = self.pacemaker.time_remaining();
            let pacemaker_timer = sleep(time_remaining);
            pin!(pacemaker_timer);

            let local_msg_future = async {
                if let Some(msg) = self.local_queue.pop_front() {
                    Some(msg)
                } else {
                    // No local messages: block forever (or until next loop iteration)
                    pending().await
                }
            };

            pin!(local_msg_future);

            tokio::select! {
                Some(msg) = to_replica_rx.recv() => {
                    match msg {
                        ReplicaInBound::HotStuff(msg) => self.handle_message(msg).await?,
                        ReplicaInBound::Transaction(tx) => {
                            self.mempool.push_back(tx);
                        }
                    }

                },

                Some(msg) = &mut local_msg_future => {
                    self.handle_message(msg).await?
                },


                _ = &mut pacemaker_timer => {
                    if self.pacemaker.should_advance_view() {

                        self.pacemaker.advance_view();
                        let outbound_msg = HotStuffMessage::new(None, Some((*self.generic_qc).clone()), self.pacemaker.curr_view, self.node_id);

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

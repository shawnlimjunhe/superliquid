use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use tokio::{
    pin,
    sync::mpsc::{self},
    time::sleep,
};

use crate::{
    config,
    hotstuff::utils,
    replica_debug, replica_log,
    state::state::LedgerState,
    types::{
        message::{ReplicaInBound, ReplicaOutbound, mpsc_error},
        transaction::{Sha256Hash, SignedTransaction},
    },
};

use super::{
    block::{Block, BlockHash},
    crypto::{PartialSig, QuorumCertificate},
    message::HotStuffMessage,
    message_window::MessageWindow,
    pacemaker::Pacemaker,
};

pub type ViewNumber = u64;

pub struct ReplicaSender {
    pub replica_tx: mpsc::Sender<ReplicaInBound>,
    pub node_tx: mpsc::Sender<ReplicaOutbound>,
}

impl ReplicaSender {
    pub(super) async fn send_to_self(&self, msg: HotStuffMessage) -> Result<(), std::io::Error> {
        self.replica_tx
            .send(ReplicaInBound::HotStuff(msg))
            .await
            .map_err(|e| mpsc_error("Send to replica failed", e))
    }

    pub(super) async fn broadcast(&self, msg: HotStuffMessage) -> Result<(), std::io::Error> {
        self.node_tx
            .send(ReplicaOutbound::Broadcast(msg))
            .await
            .map_err(|e| mpsc_error("failed to send to node", e))
    }

    pub(super) async fn send_to_node(
        &self,
        node_id: usize,
        msg: HotStuffMessage,
    ) -> Result<(), std::io::Error> {
        self.node_tx
            .send(ReplicaOutbound::SendTo(node_id, msg))
            .await
            .map_err(|e| mpsc_error("failed to send to node", e))
    }
}

pub struct HotStuffReplica {
    pub node_id: usize,
    pub validator_set: HashSet<VerifyingKey>,
    signing_key: SigningKey,

    generic_qc: Arc<QuorumCertificate>,
    locked_qc: Arc<QuorumCertificate>,

    current_proposal: Option<Block>,
    mempool: VecDeque<SignedTransaction>,
    pending_transactions: HashMap<Sha256Hash, SignedTransaction>,
    committed_transactions: HashMap<Sha256Hash, SignedTransaction>,

    pub messages: MessageWindow,
    pub pacemaker: Pacemaker,

    pub rep_node_channel: ReplicaSender,

    // State
    blockstore: HashMap<BlockHash, Block>,
    ledger_state: LedgerState,

    // view specifc flag: for view idempotency
    proposed_for_curr_view: bool,
    voted_for_curr_view: bool,
}

impl HotStuffReplica {
    pub fn new(
        node_id: usize,
        replica_tx: mpsc::Sender<ReplicaInBound>,
        node_tx: mpsc::Sender<ReplicaOutbound>,
    ) -> Self {
        let mut signing_key = config::retrieve_signing_key_checked(node_id);

        let (genesis_block, genesis_qc) = Block::create_genesis_block(&mut signing_key);
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
            pending_transactions: HashMap::new(),
            committed_transactions: HashMap::new(),

            messages: MessageWindow::new(0),

            pacemaker: Pacemaker::new(),
            rep_node_channel: ReplicaSender {
                replica_tx,
                node_tx,
            },

            ledger_state: LedgerState::new(),

            proposed_for_curr_view: false,
            voted_for_curr_view: false,
        }
    }

    fn reset_view_flags(&mut self) {
        self.proposed_for_curr_view = false;
        self.voted_for_curr_view = false;
    }

    fn advance_view(&mut self) {
        self.pacemaker.advance_view();
        self.reset_view_flags();
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
    ) -> HotStuffMessage {
        let mut message = HotStuffMessage::new(
            Some(node.clone()),
            option_justify,
            self.pacemaker.curr_view,
            self.node_id,
            self.pacemaker.curr_view,
        );
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

    /// Try to build QC(view) once any block‐hash has n‑f signatures.
    /// Returns None if no such QC exists yet.
    pub fn try_create_qc_for_view(&self, view: ViewNumber) -> Option<QuorumCertificate> {
        let msgs = self.messages.get_messages_for_view(view)?;

        // 2) Tally signatures by (block_hash → Vec<PartialSig>),
        //    verifying and deduplicating by signer.
        let mut seen: HashSet<VerifyingKey> = HashSet::new();
        let mut tally: HashMap<(BlockHash, Sha256Hash), Vec<PartialSig>> = HashMap::new();

        for m in msgs {
            if let Some(sig) = m.partial_sig.clone() {
                // a) signature must come from a known, unseen validator
                if !self.validator_set.contains(&sig.signer_id) {
                    continue;
                }

                let msg_hash = &m.hash();

                if sig
                    .signer_id
                    .verify_strict(&m.hash(), &sig.signature)
                    .is_err()
                {
                    continue;
                }

                if !seen.insert(sig.signer_id.clone()) {
                    continue;
                }

                // b) extract the vote’s block hash
                let bh = m
                    .justify
                    .as_ref()
                    .map(|qc| qc.block_hash) // if votes carry a QC
                    .or_else(|| m.node.as_ref().map(|b| b.hash()))
                    .unwrap_or([1; 32]);

                // ensure that quorum has same msg_hash
                tally.entry((bh, *msg_hash)).or_default().push(sig);
            }
        }

        // 3) As soon as any bucket reaches quorum, build the QC and return
        let quorum = self.quorum_threshold();
        for ((block_hash, message_hash), sigs) in tally {
            if sigs.len() >= quorum {
                return Some(QuorumCertificate::from_signatures(
                    view,
                    block_hash,
                    message_hash,
                    sigs,
                ));
            }
        }

        None
    }

    pub fn safe_node(&self, block: &Block, qc: &QuorumCertificate) -> bool {
        let locked_qc = &self.locked_qc.clone();
        let locked_block_hash = locked_qc.block_hash;
        let extends = block.extends_from(locked_block_hash, &self.blockstore);
        let newer_qc = qc.view_number > locked_qc.view_number;

        extends || newer_qc
    }

    fn select_transactions(&mut self) -> SignedTransaction {
        while self.mempool.len() > 0 {
            let transactions_opt = self.mempool.pop_front();
            let Some(txn) = transactions_opt else {
                return SignedTransaction::create_empty_signed_transaction(&mut self.signing_key);
            };

            let txn_hash = txn.hash();
            if self.pending_transactions.contains_key(&txn_hash)
                || self.committed_transactions.contains_key(&txn_hash)
            {
                continue;
            }
            return txn;
        }
        return SignedTransaction::create_empty_signed_transaction(&mut self.signing_key);
    }

    fn leader_create_message(&mut self, new_block: Block) -> HotStuffMessage {
        let outbound_msg = HotStuffMessage::new(
            Some(new_block),
            None,
            self.pacemaker.curr_view,
            self.node_id,
            self.pacemaker.curr_view,
        );
        // let partial_sig = self.sign(&outbound_msg);
        // outbound_msg.partial_sig = Some(partial_sig);
        return outbound_msg;
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

    pub fn leader_handle_message(&mut self) -> Option<HotStuffMessage> {
        let curr_view = self.pacemaker.curr_view;
        replica_log!(
            self.node_id,
            "Leader handle message at view: {:?}",
            curr_view
        );
        let selected_transactions = &self.select_transactions();

        self.pacemaker.reset_timer();

        let is_new_view = utils::has_quorum_for_new_view(
            self.messages.get_messages_for_view(curr_view - 1),
            self.pacemaker.curr_view - 1,
            self.quorum_threshold(),
        );

        if is_new_view {
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
                        self.messages.prune_before_view(self.generic_qc.view_number);
                    }
                }
                None => {}
            };

            let Some(parent) = self.blockstore.get(&self.generic_qc.block_hash) else {
                // cant find QC's block
                replica_debug!(
                    self.node_id,
                    self.pacemaker.curr_view,
                    "cant find qc's block: L"
                );
                return None;
            };

            let curr_view = self.pacemaker.curr_view;
            let new_block = Block::create_leaf(
                parent,
                selected_transactions.clone(),
                curr_view,
                (*self.generic_qc).clone(),
            );

            self.blockstore.insert(new_block.hash(), new_block.clone());

            self.current_proposal = Some(new_block.clone());
            replica_debug!(
                self.node_id,
                self.pacemaker.curr_view,
                "Success: Leader:: Prepare"
            );

            // leader should also vote for their own message
            return Some(self.leader_create_message(new_block));
        }
        // is not new view

        let has_quorum_votes = utils::has_quorum_votes_for_view(
            self.messages.get_messages_for_view(curr_view - 1),
            self.pacemaker.curr_view - 1,
            self.quorum_threshold(),
        );

        if has_quorum_votes {
            // Attempt to form QC from votes from prev view

            let qc = self.try_create_qc_for_view(curr_view - 1);

            match qc {
                Some(qc) => {
                    self.generic_qc = Arc::new(qc);
                    self.messages.prune_before_view(self.generic_qc.view_number);
                    replica_debug!(self.node_id, self.pacemaker.curr_view, "Able to form QC");
                }

                None => {
                    // Unable to form a QC, continue propose new block justified from highest qc or generic qc
                    replica_debug!(self.node_id, self.pacemaker.curr_view, "Unable to form QC");
                    return None;
                }
            }
        }

        replica_debug!(
            self.node_id,
            self.pacemaker.curr_view,
            "waiting for quorum (n - f)",
        );
        return None;
    }

    pub fn replica_handle_message(&mut self, msg: HotStuffMessage) -> Option<HotStuffMessage> {
        replica_log!(
            self.node_id,
            "Replica handle message with at view: {:?}",
            self.pacemaker.curr_view
        );

        let curr_view = self.pacemaker.curr_view;

        if msg.sender != self.pacemaker.get_leader_for_view(self.pacemaker.curr_view) {
            // could be a pending vote for the current proposal
            replica_debug!(
                self.node_id,
                self.pacemaker.curr_view,
                "msg sender: {:?} is not the leader. Leader is: {:?}",
                msg.sender,
                self.pacemaker.get_leader_for_view(curr_view)
            );
            return None;
        }

        if msg.partial_sig.is_some() {
            // we can optimistically fast forward here - TODO
            // vote message from leader for next view
            return None;
        }

        // b*
        let Some(b_star) = msg.node.clone() else {
            replica_debug!(self.node_id, self.pacemaker.curr_view, "No node in message");
            return None;
        };
        self.blockstore.insert(b_star.hash(), b_star.clone());

        // b″ := b*.justify.node
        let (Some(b_double_prime), Some(b_star_justify)) =
            self.get_justifed_block_and_qc_cloned(&b_star)
        else {
            replica_debug!(
                self.node_id,
                self.pacemaker.curr_view,
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
            outbound_msg = Some(self.vote_message(&b_star, None));
            replica_debug!(self.node_id, self.pacemaker.curr_view, "voting for block",);
        } else {
            replica_debug!(
                self.node_id,
                self.pacemaker.curr_view,
                "is_safe: {:?}, is_valid_sig: {:?}",
                is_safe,
                is_valid_sig
            );
            if !is_valid_sig {
                println!("{:?}", &b_star_justify)
            }
            return outbound_msg;
        }

        replica_debug!(
            self.node_id,
            self.pacemaker.curr_view,
            "Start pre-commit phase on b*'s parent",
        );

        if !self.is_parent(&b_star, &b_double_prime) {
            return outbound_msg;
        }
        self.generic_qc = Arc::new(b_star_justify.clone());

        self.messages.prune_before_view(self.generic_qc.view_number);

        replica_debug!(
            self.node_id,
            self.pacemaker.curr_view,
            "Start commit phase on b*'s grandparent",
        );

        // b′ := b″.justify.node
        let (Some(b_prime), Some(b_double_prime_justify)) =
            self.get_justifed_block_and_qc_cloned(&b_double_prime)
        else {
            replica_debug!(
                self.node_id,
                self.pacemaker.curr_view,
                "Missing b′ justified by b″"
            );
            return outbound_msg;
        };

        if !self.is_parent(&b_double_prime, &b_prime) {
            return outbound_msg;
        }

        self.locked_qc = Arc::new(b_double_prime_justify);
        self.pacemaker
            .set_last_committed_view(self.locked_qc.clone());

        // b := b′.justify.node
        let commited_block = {
            let Some(b) = self.get_justified_block(&b_prime) else {
                replica_debug!(
                    self.node_id,
                    self.pacemaker.curr_view,
                    "Missing b justified by b′"
                );
                return outbound_msg;
            };

            if !self.is_parent(&b_prime, b) {
                return outbound_msg;
            }
            b.clone()
        };

        replica_log!(self.node_id, "Commit success on view: {:?}", curr_view);

        self.ledger_state.apply_block(&commited_block);

        return outbound_msg;
    }

    fn sync_view(&mut self, msg: &HotStuffMessage) -> bool {
        let incoming_view = msg.view_number;
        self.pacemaker.fast_forward_view(incoming_view)
    }

    async fn handle_message(&mut self, msg: HotStuffMessage) -> Result<(), std::io::Error> {
        if self.sync_view(&msg) {
            replica_debug!(
                self.node_id,
                self.pacemaker.curr_view,
                "Recieved higher view message from node: {:?}",
                msg.sender
            );
            self.reset_view_flags();
            // advance view if view is behind
            let is_leader = self.pacemaker.current_leader() == self.node_id;
            if is_leader {
                // Send new-view msg to self
                let new_view_msg: HotStuffMessage = self.create_new_view();
                self.rep_node_channel.send_to_self(new_view_msg).await?
            }
        }

        if msg.view_number + 1 < self.pacemaker.curr_view {
            replica_debug!(
                self.node_id,
                self.pacemaker.curr_view,
                "Recieved stale message from view: {:?} from node: {:?}",
                msg.view_number,
                msg.sender
            );
        }

        replica_debug!(
            self.node_id,
            self.pacemaker.curr_view,
            "Recieved message: {:?} with view: {:?} from node-id: {:?}",
            msg.partial_sig,
            msg.view_number,
            msg.sender,
        );

        self.messages.push(msg.clone());

        let leader = self.pacemaker.current_leader();
        let is_leader = self.node_id == leader;
        let curr_view = self.pacemaker.curr_view;

        // Choose which role-specific handler to run:
        // Every node executes replica logic
        // Leader executes leader logic and sends message to itself to handle as replica
        if is_leader && !self.proposed_for_curr_view {
            let leader_outbound_msg_opt = self.leader_handle_message();
            let Some(leader_outbound_msg) = leader_outbound_msg_opt else {
                return Ok(());
            };

            self.rep_node_channel
                .broadcast(leader_outbound_msg.clone())
                .await?;
            self.proposed_for_curr_view = true;

            // handle leader's msg as replica
            // Dont send to same channel to prevent data race
            self.messages.push(leader_outbound_msg.clone());
            let replica_outbound_msg_opt = self.replica_handle_message(leader_outbound_msg.clone());

            let Some(replica_outbound_msg) = replica_outbound_msg_opt else {
                return Ok(());
            };
            let next_leader = self
                .pacemaker
                .get_leader_for_view(self.pacemaker.curr_view + 1);

            replica_debug!(
                self.node_id,
                self.pacemaker.curr_view,
                "Sending to next leader: {:?} with msg view: {:?}",
                next_leader,
                replica_outbound_msg.view_number
            );

            self.rep_node_channel
                .send_to_node(next_leader, replica_outbound_msg)
                .await?;

            self.voted_for_curr_view = true;

            return Ok(());
        }

        if self.voted_for_curr_view {
            return Ok(());
        }

        // Handle as replica only
        let outbound_msg_opt = self.replica_handle_message(msg);

        let Some(outbound_msg) = outbound_msg_opt else {
            return Ok(());
        };

        let next_leader = self.pacemaker.get_leader_for_view(curr_view + 1);

        if self.node_id == next_leader {
            replica_debug!(
                self.node_id,
                self.pacemaker.curr_view,
                "Sending to self as node is next leader"
            );
            let _ = self.rep_node_channel.send_to_self(outbound_msg).await;

            self.voted_for_curr_view = true;

            return Ok(());
        }

        replica_debug!(
            self.node_id,
            self.pacemaker.curr_view,
            "Sending to next leader: {:?}",
            next_leader,
        );

        self.rep_node_channel
            .send_to_node(next_leader, outbound_msg)
            .await?;
        Ok(())
    }

    fn create_new_view(&mut self) -> HotStuffMessage {
        HotStuffMessage::new(
            None,
            Some((*self.generic_qc).clone()),
            self.pacemaker.curr_view - 1,
            self.node_id,
            self.pacemaker.curr_view - 1,
        )
    }

    pub async fn run_replica(
        &mut self,
        mut to_replica_rx: mpsc::Receiver<ReplicaInBound>,
    ) -> Result<(), std::io::Error> {
        replica_log!(self.node_id, "Running replica...");
        loop {
            // Refresh pacemaker timer dynamically each loop
            let time_remaining = self.pacemaker.time_remaining();
            let pacemaker_timer = sleep(time_remaining);
            pin!(pacemaker_timer);

            tokio::select! {
                Some(msg) = to_replica_rx.recv() => {
                    match msg {
                        ReplicaInBound::HotStuff(msg) => self.handle_message(msg).await?,
                        ReplicaInBound::Transaction(tx) => {
                            self.mempool.push_back(tx);
                        }
                    }
                },

                _ = &mut pacemaker_timer => {
                    if self.pacemaker.should_advance_view() {

                        self.advance_view();
                        let leader = self.pacemaker.current_leader();
                        let outbound_msg = self.create_new_view();

                        if self.node_id == leader {
                            let _ = self.rep_node_channel.send_to_self(outbound_msg).await;
                            replica_debug!(self.node_id, self.pacemaker.curr_view -1 , "Timeout: Sending self");
                        } else {
                            replica_debug!(self.node_id, self.pacemaker.curr_view -1 , "Timeout: Sending to node: {:?} with msg view: {:?}", leader, outbound_msg.view_number);
                            self.rep_node_channel.send_to_node(leader, outbound_msg).await?;
                        }

                    }
                }
            }
        }
    }
}

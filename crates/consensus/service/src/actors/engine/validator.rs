//! Validator engine request routing.

use std::{sync::Arc, time::Instant};

use alloy_eips::BlockNumberOrTag;
use base_consensus_engine::{
    ConsolidateTask, EngineClient, EngineTask, FinalizeTask, Metrics as EngineMetrics,
};
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{error, warn};

use crate::{
    EngineActorRequest, EngineClientError, EngineDerivationClient, EngineError, EngineProcessor,
    EngineRequestReceiver, InsertUnsafePayloadRequest, ReconcileShadowRequest, ResetRequestOutcome,
};

impl<EngineClient_, DerivationClient> EngineRequestReceiver
    for EngineProcessor<EngineClient_, DerivationClient>
where
    EngineClient_: EngineClient + 'static,
    DerivationClient: EngineDerivationClient + 'static,
{
    fn start(
        mut self,
        mut request_channel: mpsc::Receiver<EngineActorRequest>,
    ) -> JoinHandle<Result<(), EngineError>> {
        tokio::spawn(async move {
            let reth_head = self.client().l2_block_info_by_label(BlockNumberOrTag::Latest).await;
            if let Err(error) = &reth_head {
                warn!(target: "engine", ?error, "Bootstrap: failed to query reth head");
            }
            self.bootstrap_validator(reth_head.ok().flatten()).await;

            loop {
                let _iter_timer =
                    base_metrics::timed!(EngineMetrics::engine_processor_iteration_duration());
                base_metrics::time!(EngineMetrics::engine_processor_drain_duration_seconds(), {
                    self.drain().await.inspect_err(
                        |error| error!(target: "engine", ?error, "Failed to drain engine tasks"),
                    )
                })?;

                let request = base_metrics::time!(
                    EngineMetrics::engine_processor_recv_wait_duration_seconds(),
                    { request_channel.recv().await }
                );
                let Some(request) = request else {
                    error!(target: "engine", "Engine processing request receiver closed unexpectedly");
                    return Err(EngineError::ChannelClosed);
                };

                match request {
                    EngineActorRequest::BuildRequest(request) => {
                        self.process_build_request(*request).await?;
                    }
                    EngineActorRequest::GetPayloadRequest(request) => {
                        self.process_get_payload_request(*request).await?;
                    }
                    EngineActorRequest::ProcessSafeL2SignalRequest(safe_signal) => {
                        self.enqueue(EngineTask::Consolidate(Box::new(ConsolidateTask::new(
                            Arc::clone(self.client()),
                            Arc::clone(self.rollup()),
                            safe_signal,
                        ))));
                    }
                    EngineActorRequest::ProcessFinalizedL2BlockNumberRequest(block_number) => {
                        self.enqueue(EngineTask::Finalize(Box::new(FinalizeTask::new(
                            Arc::clone(self.client()),
                            Arc::clone(self.rollup()),
                            *block_number,
                        ))));
                    }
                    EngineActorRequest::ProcessUnsafeL2BlockRequest(envelope) => {
                        self.handle_external_unsafe_l2_block(*envelope);
                    }
                    EngineActorRequest::ProcessAdminUnsafeL2BlockRequest(envelope) => {
                        self.handle_admin_unsafe_l2_block(*envelope);
                    }
                    EngineActorRequest::ProcessLocalUnsafeL2BlockRequest(request) => {
                        let InsertUnsafePayloadRequest { envelope, result_tx, otel_cx } = *request;
                        let _guard = otel_cx.attach();
                        self.handle_local_unsafe_l2_block(envelope, result_tx);
                    }
                    EngineActorRequest::ReconcileShadowRequest(request) => {
                        let ReconcileShadowRequest { result_tx, .. } = *request;
                        if result_tx
                            .send(Err(EngineClientError::ShadowReconciliationDisabled))
                            .await
                            .is_err()
                        {
                            warn!(target: "engine", "Shadow reconciliation response receiver dropped");
                        }
                    }
                    EngineActorRequest::ResetRequest(request) => {
                        let reset_started = Instant::now();
                        let unsafe_before = self.engine_state().sync_state.unsafe_head();
                        if !self.engine_state().el_sync_finished {
                            warn!(target: "engine", "Deferring engine reset: EL sync not yet complete");
                            request
                                .respond(
                                    reset_started,
                                    unsafe_before,
                                    unsafe_before,
                                    ResetRequestOutcome::Deferred,
                                    Err(EngineClientError::ELSyncing),
                                )
                                .await;
                            continue;
                        }

                        warn!(target: "engine", "Received reset request");
                        match self.reset_engine_state().await {
                            Ok(safe_head) => {
                                let response = self
                                    .notify_derivation_of_reset(safe_head)
                                    .await
                                    .map_err(|error| {
                                        EngineClientError::ResetForkchoiceError(error.to_string())
                                    });
                                let unsafe_after = self.engine_state().sync_state.unsafe_head();
                                let outcome = if response.is_ok() {
                                    ResetRequestOutcome::from_unsafe_heads(
                                        unsafe_before,
                                        unsafe_after,
                                    )
                                } else {
                                    ResetRequestOutcome::DerivationNotificationFailed
                                };
                                request
                                    .respond(
                                        reset_started,
                                        unsafe_before,
                                        unsafe_after,
                                        outcome,
                                        response,
                                    )
                                    .await;
                            }
                            Err(error) => {
                                let response =
                                    Err(EngineClientError::ResetForkchoiceError(error.to_string()));
                                if !request
                                    .respond(
                                        reset_started,
                                        unsafe_before,
                                        self.engine_state().sync_state.unsafe_head(),
                                        ResetRequestOutcome::Failed,
                                        response,
                                    )
                                    .await
                                {
                                    return Err(error);
                                }
                            }
                        }
                    }
                }
            }
        })
    }
}

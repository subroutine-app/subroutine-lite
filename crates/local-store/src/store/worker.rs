use chrono::{DateTime, Utc};
use std::sync::mpsc;
use subroutine_core::{
    AllData, ApiErrorBody, ClientMutation, DataDelta, MutationReceipt, OptimisticPatch,
};
use uuid::Uuid;

use crate::{
    OutboxEntry, Projection, Result, StaleResolution, StoreDiagnostics, SyncState, db::Database,
};

pub(super) type Reply<T> = mpsc::SyncSender<Result<T>>;

pub(super) enum Command {
    State(Reply<SyncState>),
    Projection(Reply<Projection>),
    InstallSnapshot(Box<AllData>, SyncState, Reply<Projection>),
    ApplyDelta(Box<DataDelta>, Reply<Projection>),
    ApplyLocalPatch(Box<OptimisticPatch>, Reply<Projection>),

    SetEventBusyOverride(Uuid, Option<bool>, Reply<Projection>),
    Enqueue(Box<ClientMutation>, Reply<Projection>),
    OldestOutbox(Reply<Option<OutboxEntry>>),
    NextSendable(DateTime<Utc>, Reply<Option<OutboxEntry>>),
    RebaseUnsealed(Uuid, i64, Reply<()>),
    ResolveStaleDelta {
        mutation_id: Uuid,
        replacement_mutation_id: Uuid,
        since: i64,
        delta: Box<DataDelta>,
        reply: Reply<StaleResolution>,
    },
    ResolveStaleSnapshot {
        mutation_id: Uuid,
        data: Box<AllData>,
        expected: SyncState,
        reply: Reply<StaleResolution>,
    },
    RecordRetry(Uuid, DateTime<Utc>, String, Reply<()>),
    RecordReceipt(Box<MutationReceipt>, Reply<()>),
    Block(Uuid, Box<ApiErrorBody>, Reply<Projection>),
    RetryBlocked(Uuid, Uuid, Reply<Projection>),
    RetryCompatibilityRejection(Uuid, Reply<Projection>),
    DiscardBlocked(Uuid, Reply<Projection>),
    PurgeRemote(Reply<()>),

    Diagnostics(Reply<StoreDiagnostics>),
    Shutdown,
}

impl Command {
    pub(super) fn handle(self, database: &mut Database) -> bool {
        match self {
            Command::State(reply) => respond(reply, database.state()),
            Command::Projection(reply) => respond(reply, database.projection()),
            Command::InstallSnapshot(data, expected, reply) => {
                respond(reply, database.install_snapshot(*data, expected))
            }
            Command::ApplyDelta(delta, reply) => respond(reply, database.apply_delta(*delta)),
            Command::ApplyLocalPatch(patch, reply) => {
                respond(reply, database.apply_local_patch(*patch))
            }

            Command::SetEventBusyOverride(event_id, busy_override, reply) => respond(
                reply,
                database.set_event_busy_override(event_id, busy_override),
            ),
            Command::Enqueue(mutation, reply) => respond(reply, database.enqueue(*mutation)),
            Command::OldestOutbox(reply) => respond(reply, database.oldest_outbox()),
            Command::NextSendable(now, reply) => respond(reply, database.next_sendable(now)),
            Command::RebaseUnsealed(mutation_id, base_seq, reply) => {
                respond(reply, database.rebase_unsealed(mutation_id, base_seq))
            }
            Command::ResolveStaleDelta {
                mutation_id,
                replacement_mutation_id,
                since,
                delta,
                reply,
            } => respond(
                reply,
                database.resolve_stale_delta(mutation_id, replacement_mutation_id, since, *delta),
            ),
            Command::ResolveStaleSnapshot {
                mutation_id,
                data,
                expected,
                reply,
            } => respond(
                reply,
                database.resolve_stale_snapshot(mutation_id, *data, expected),
            ),
            Command::RecordRetry(mutation_id, next_attempt_at, error, reply) => respond(
                reply,
                database.record_retry(mutation_id, next_attempt_at, error),
            ),
            Command::RecordReceipt(receipt, reply) => {
                respond(reply, database.record_receipt(*receipt))
            }
            Command::Block(mutation_id, error, reply) => {
                respond(reply, database.block(mutation_id, *error))
            }
            Command::RetryBlocked(mutation_id, replacement_mutation_id, reply) => respond(
                reply,
                database.retry_blocked(mutation_id, replacement_mutation_id),
            ),
            Command::RetryCompatibilityRejection(mutation_id, reply) => {
                respond(reply, database.retry_compatibility_rejection(mutation_id))
            }
            Command::DiscardBlocked(mutation_id, reply) => {
                respond(reply, database.discard_blocked(mutation_id))
            }
            Command::PurgeRemote(reply) => respond(reply, database.purge_remote()),

            Command::Diagnostics(reply) => respond(reply, database.diagnostics()),
            Command::Shutdown => return false,
        }
        true
    }
}

fn respond<T>(reply: Reply<T>, result: Result<T>) {
    let _ = reply.send(result);
}

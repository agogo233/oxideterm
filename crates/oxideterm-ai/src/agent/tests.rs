use super::*;
use crate::RuntimeOwnerKey;
use std::{
    collections::{BTreeSet, HashSet},
    time::Duration,
};

fn group(runtime: &AgentRuntime) -> AgentRunRef {
    runtime.create_group(
        "conversation".into(),
        AgentModel {
            provider_id: "provider".into(),
            model: "model".into(),
        },
        AgentScope {
            targets: HashSet::from([RuntimeOwnerKey::new()]),
            tools: BTreeSet::from(["observe_terminal".into()]),
        },
        2,
    )
}
fn child(runtime: &AgentRuntime, parent: &AgentRunRef) -> AgentRunRef {
    runtime
        .delegate(
            parent,
            AgentText::new("Inspect"),
            AgentText::new("Inspect the terminal"),
            runtime.snapshot(parent).unwrap().scope,
            None,
        )
        .unwrap()
}
fn result() -> AgentResult {
    AgentResult {
        summary: AgentText::new("Done"),
        evidence: Vec::new(),
        actions: Vec::new(),
        unfinished: Vec::new(),
        error_code: None,
    }
}

#[test]
fn usage_snapshots_replace_cumulative_updates_and_preserve_unknown_requests() {
    let runtime = AgentRuntime::new(1);
    let parent = group(&runtime);
    let first = runtime.begin_request(&parent).unwrap();
    runtime
        .record_usage(&parent, first, Some(20), Some(1))
        .unwrap();
    runtime.record_usage(&parent, first, None, Some(8)).unwrap();
    assert_eq!(
        runtime.snapshot(&parent).unwrap().usage,
        AgentUsage {
            requests: 1,
            input_tokens: Some(20),
            output_tokens: Some(8)
        }
    );
    runtime.begin_request(&parent).unwrap();
    assert_eq!(
        runtime.snapshot(&parent).unwrap().usage,
        AgentUsage {
            requests: 2,
            input_tokens: None,
            output_tokens: None
        }
    );
}

#[test]
fn completion_cannot_discard_an_accepted_supplement_or_unread_child_result() {
    let runtime = AgentRuntime::new(1);
    let parent = group(&runtime);
    let child = child(&runtime, &parent);
    runtime
        .send(
            &parent,
            &parent,
            AgentMessageKind::UserSupplement,
            AgentText::new("Also check disk space"),
        )
        .unwrap();
    runtime.complete(&child, result()).unwrap();
    assert_eq!(
        runtime.prepare_completion(&parent, 0),
        Err(AgentError::PendingMessages)
    );
    assert_eq!(runtime.drain_messages(&parent).unwrap().len(), 1);
    assert_eq!(
        runtime.prepare_completion(&parent, 0),
        Err(AgentError::PendingMessages)
    );
    let cursor = runtime
        .updates_since(&parent, 0)
        .unwrap()
        .last()
        .unwrap()
        .sequence;
    runtime.prepare_completion(&parent, cursor).unwrap();
    assert!(
        runtime
            .send(
                &parent,
                &parent,
                AgentMessageKind::UserSupplement,
                AgentText::new("late instruction")
            )
            .is_err()
    );
    assert!(!runtime.accepts_messages(&parent));
    runtime.complete(&parent, result()).unwrap();
    runtime.finish_group(&parent).unwrap();
}

#[test]
fn persisted_agents_are_descriptive_and_deleted_with_their_conversation() {
    let directory = tempfile::tempdir().unwrap();
    let store = crate::AiChatPersistenceStore::new(directory.path().join("agents.redb"));
    let mut state = crate::AiChatState::default();
    state.create_conversation("conversation".into(), Some("Parent".into()), 1, None);
    store.save_state(state.clone()).unwrap();
    let runtime = AgentRuntime::new(1);
    let parent = group(&runtime);
    let child = child(&runtime, &parent);
    let record = AgentRecord {
        parent_usage: AgentUsage::default(),
        created_at_ms: 2,
        snapshot: runtime.snapshot(&child).unwrap(),
        parent_message_id: "reply".into(),
        target_labels: vec![AgentText::new("server")],
        messages: vec![serde_json::from_value(serde_json::json!({
            "id": "child-answer", "role": "assistant", "content": "Visible answer", "timestamp_ms": 2,
            "thinking_content": "private reasoning",
            "turn": { "parts": [
                { "type": "thinking", "text": "private reasoning" },
                { "type": "text", "text": "Visible answer" }
            ] }
        })).unwrap()],
        communication: Vec::new(),
        revision: 1,
    };
    store.save_agent_records(vec![record.clone()]).unwrap();
    let summaries = store.load_agent_summaries("conversation").unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].snapshot.state, AgentState::Interrupted);
    assert!(summaries[0].snapshot.scope.targets.is_empty());
    assert_eq!(state.conversations.len(), 1);
    let loaded = store
        .load_agent_record("conversation", &child.run_id)
        .unwrap()
        .unwrap();
    let message = &loaded.messages[0];
    assert!(message.thinking_content.is_none());
    assert_eq!(
        message.turn.as_ref().unwrap()["parts"],
        serde_json::json!([
            { "type": "text", "text": "Visible answer" }
        ])
    );
    state.delete_conversation("conversation");
    store.save_state(state).unwrap();
    store.save_agent_records(vec![record]).unwrap();
    assert!(
        store
            .load_agent_record("conversation", &child.run_id)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .load_agent_summaries("conversation")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn waiting_releases_capacity_and_model_stays_locked_until_followup() {
    let runtime = AgentRuntime::new(1);
    let parent = group(&runtime);
    let first = child(&runtime, &parent);
    let second = child(&runtime, &parent);
    let resources = AgentResourceCoordinator::default();
    let mut execution =
        AgentExecution::new(runtime.clone(), first.clone(), resources.clone()).unwrap();
    execution.ready().await.unwrap();
    runtime.lock_model(&first).unwrap();
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<()>();
    let waiting =
        tokio::spawn(async move { execution.wait(AgentState::AwaitingParent, reply_rx).await });
    let mut other = AgentExecution::new(runtime.clone(), second.clone(), resources).unwrap();
    tokio::time::timeout(Duration::from_secs(1), other.ready())
        .await
        .unwrap()
        .unwrap();
    runtime.set_state(&first, AgentState::Queued).unwrap();
    let alternate = AgentModel {
        provider_id: "configured".into(),
        model: "other".into(),
    };
    assert_eq!(
        runtime.change_queued_model(&first, alternate.clone()),
        Err(AgentError::InvalidState)
    );
    runtime.stop(&parent, &first).unwrap();
    assert!(waiting.await.unwrap().is_err());
    drop(reply_tx);
    let resumed = runtime.resume(&parent, &first).unwrap();
    runtime
        .change_queued_model(&resumed, alternate.clone())
        .unwrap();
    assert_eq!(runtime.lock_model(&resumed).unwrap(), alternate);
    assert_eq!(
        runtime.snapshot(&second).unwrap().state,
        AgentState::Running
    );
}

#[tokio::test]
async fn terminal_response_and_interactive_input_do_not_release_running_command() {
    let runtime = AgentRuntime::new(2);
    let parent = group(&runtime);
    let first = child(&runtime, &parent);
    let resources = AgentResourceCoordinator::default();
    let key = RuntimeOwnerKey::new();
    let lease = resources
        .acquire(
            key.clone(),
            first.clone(),
            runtime.cancellation(&first).unwrap(),
        )
        .await
        .unwrap();
    let command = AgentToolLease::new(resources.clone(), lease.clone());
    command.dispatched();
    command.monitor_command();
    command.finish_response(true);
    drop(AgentToolResponse(vec![command.clone()]));
    assert!(resources.owns(&lease));
    drop(AgentToolLease::borrow_command(
        resources.clone(),
        lease.clone(),
    ));
    assert!(resources.owns(&lease));
    let input = AgentToolLease::borrow_command(
        resources.clone(),
        resources.owned_by(&key, &first).unwrap(),
    );
    input.dispatched();
    input.finish_response(true);
    drop(AgentToolResponse(vec![input]));
    assert!(resources.owns(&lease));
    command.command_finished();
    assert!(!resources.has_owner(&key));
    assert!(!resources.is_blocked(&key));
}

#[tokio::test]
async fn dropped_mutation_blocks_reuse_but_undispatched_work_does_not() {
    let runtime = AgentRuntime::new(1);
    let parent = group(&runtime);
    let resources = AgentResourceCoordinator::default();
    let key = RuntimeOwnerKey::new();
    let lease = resources
        .acquire(
            key.clone(),
            parent.clone(),
            runtime.cancellation(&parent).unwrap(),
        )
        .await
        .unwrap();
    drop(AgentToolLease::new(resources.clone(), lease));
    let lease = resources
        .acquire(
            key.clone(),
            parent.clone(),
            runtime.cancellation(&parent).unwrap(),
        )
        .await
        .unwrap();
    let tool = AgentToolLease::new(resources.clone(), lease);
    tool.dispatched();
    drop(AgentToolResponse(vec![tool]));
    assert!(matches!(
        resources
            .acquire(key, parent.clone(), runtime.cancellation(&parent).unwrap())
            .await,
        Err(AgentError::ResourceUnresolved)
    ));
}

#[tokio::test]
async fn mailbox_isolated_and_resume_rejects_late_results() {
    let runtime = AgentRuntime::new(2);
    let parent = group(&runtime);
    let first = child(&runtime, &parent);
    let second = child(&runtime, &parent);
    assert_eq!(
        runtime.send(
            &first,
            &second,
            AgentMessageKind::Question,
            AgentText::new("cross talk")
        ),
        Err(AgentError::ParentOnly)
    );
    assert_eq!(
        runtime.delegate(
            &first,
            AgentText::default(),
            AgentText::default(),
            AgentScope::default(),
            None
        ),
        Err(AgentError::ParentOnly)
    );
    runtime
        .send(
            &first,
            &parent,
            AgentMessageKind::Question,
            AgentText::new("Which process?"),
        )
        .unwrap();
    let received = runtime.wait_messages(&parent).await.unwrap();
    assert_eq!(received.len(), 1);
    assert!(received[0].consumed);
    assert!(runtime.drain_messages(&parent).unwrap().is_empty());
    runtime.complete(&first, result()).unwrap();
    let resumed = runtime.resume(&parent, &first).unwrap();
    assert_ne!(resumed.run_id, first.run_id);
    assert_eq!(
        runtime.complete(&first, result()),
        Err(AgentError::StaleRun)
    );
    assert_eq!(runtime.finish_group(&parent), Err(AgentError::InvalidState));
}

#[tokio::test]
async fn concurrency_and_budget_are_shared_and_waiters_cancel() {
    let runtime = AgentRuntime::new(1);
    let parent = group(&runtime);
    let first = child(&runtime, &parent);
    let second = child(&runtime, &parent);
    let permit = runtime.acquire(&first).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(10), runtime.acquire(&second))
            .await
            .is_err()
    );
    runtime.take_round(&parent).unwrap();
    runtime.take_round(&first).unwrap();
    assert_eq!(
        runtime.take_round(&second),
        Err(AgentError::BudgetExhausted)
    );
    runtime.stop(&parent, &second).unwrap();
    assert!(matches!(
        runtime.acquire(&second).await,
        Err(AgentError::Cancelled)
    ));
    assert!(matches!(
        runtime.wait_messages(&second).await,
        Err(AgentError::Cancelled)
    ));
    drop(permit);
    assert_eq!(runtime.snapshot(&first).unwrap().state, AgentState::Running);
}

#[tokio::test]
async fn resource_timeout_retains_ownership_and_takeover_rejects_late_completion() {
    let runtime = AgentRuntime::new(2);
    let parent = group(&runtime);
    let first = child(&runtime, &parent);
    let second = child(&runtime, &parent);
    let resources = AgentResourceCoordinator::default();
    let resource = runtime
        .snapshot(&parent)
        .unwrap()
        .scope
        .targets
        .into_iter()
        .next()
        .unwrap();
    let lease = resources
        .acquire(
            resource.clone(),
            first.clone(),
            runtime.cancellation(&first).unwrap(),
        )
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(10),
            resources.acquire(
                resource.clone(),
                second.clone(),
                runtime.cancellation(&second).unwrap()
            )
        )
        .await
        .is_err()
    );
    assert!(resources.owns(&lease));
    assert_eq!(resources.invalidate(&resource), Some(first));
    assert!(matches!(
        resources
            .acquire(
                resource.clone(),
                second.clone(),
                runtime.cancellation(&second).unwrap()
            )
            .await,
        Err(AgentError::ResourceUnresolved)
    ));
    resources.allow_new_requests(&resource);
    let next = resources
        .acquire(
            resource.clone(),
            second.clone(),
            runtime.cancellation(&second).unwrap(),
        )
        .await
        .unwrap();
    assert!(!resources.complete(&lease));
    assert!(resources.owns(&next));
    assert!(resources.complete(&next));
}

#[test]
fn delegated_scope_and_secret_boundaries() {
    let runtime = AgentRuntime::new(2);
    let parent = group(&runtime);
    let mut scope = runtime.snapshot(&parent).unwrap().scope;
    scope.targets.insert(RuntimeOwnerKey::new());
    assert_eq!(
        runtime.delegate(
            &parent,
            AgentText::default(),
            AgentText::default(),
            scope,
            None
        ),
        Err(AgentError::ScopeDenied)
    );
    let text = AgentText::new("Authorization: Bearer private-token-value");
    assert!(!format!("{text:?}").contains("private-token-value"));
    assert!(
        !serde_json::to_string(&text)
            .unwrap()
            .contains("private-token-value")
    );
    let first = child(&runtime, &parent);
    let saved = runtime.snapshot(&first).unwrap();
    let serialized = serde_json::to_string(&saved).unwrap();
    for target in &saved.scope.targets {
        assert!(!serialized.contains(target.as_str()));
    }
    let restored: AgentSnapshot = serde_json::from_str(&serialized).unwrap();
    assert_eq!(restored.state, AgentState::Interrupted);
    assert!(restored.scope.targets.is_empty());
    assert!(restored.scope.tools.is_empty());
}

#[tokio::test]
async fn parent_receives_all_completions_even_when_message_mailbox_is_full() {
    let runtime = AgentRuntime::new(2);
    let parent = group(&runtime);
    let first = child(&runtime, &parent);
    let second = child(&runtime, &parent);
    for _ in 0..super::runtime::MAX_MAILBOX_MESSAGES {
        runtime
            .send(
                &first,
                &parent,
                AgentMessageKind::Question,
                AgentText::new("Need a target"),
            )
            .unwrap();
    }
    assert_eq!(
        runtime.send(
            &first,
            &parent,
            AgentMessageKind::Question,
            AgentText::new("Overflow")
        ),
        Err(AgentError::MailboxFull)
    );
    let cursor = runtime
        .wait_updates(&parent, 0)
        .await
        .unwrap()
        .last()
        .unwrap()
        .sequence;
    runtime.complete(&first, result()).unwrap();
    runtime.complete(&second, result()).unwrap();
    let updates = runtime.wait_updates(&parent, cursor).await.unwrap();
    assert_eq!(
        updates
            .iter()
            .map(|update| &update.source)
            .collect::<Vec<_>>(),
        vec![&first, &second]
    );
    assert!(
        runtime
            .wait_updates(&parent, updates.last().unwrap().sequence)
            .await
            .unwrap()
            .is_empty()
    );
    runtime.finish_group(&parent).unwrap();
    assert_eq!(
        runtime.resume(&parent, &first),
        Err(AgentError::GroupFinished)
    );
}

#[tokio::test]
async fn progress_does_not_return_from_parent_wait_and_question_does() {
    let runtime = AgentRuntime::new(2);
    let parent = group(&runtime);
    let first = child(&runtime, &parent);
    let waiting = runtime.wait_updates(&parent, 0);
    tokio::pin!(waiting);
    assert!(futures_util::poll!(&mut waiting).is_pending());
    runtime
        .report_progress(&first, AgentText::new("Inspecting"))
        .unwrap();
    assert!(futures_util::poll!(&mut waiting).is_pending());
    runtime
        .send(
            &first,
            &parent,
            AgentMessageKind::Question,
            AgentText::new("Which service?"),
        )
        .unwrap();
    let updates = waiting.await.unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(
        updates[0].message.as_ref().unwrap().kind,
        AgentMessageKind::Question
    );
}

#[tokio::test]
async fn changing_concurrency_and_cancelling_a_queued_future_preserves_progress() {
    let runtime = AgentRuntime::new(1);
    let parent = group(&runtime);
    let first = child(&runtime, &parent);
    let second = child(&runtime, &parent);
    let third = child(&runtime, &parent);
    let permit = runtime.acquire(&first).await.unwrap();
    assert!(matches!(
        runtime.acquire(&first).await,
        Err(AgentError::InvalidState)
    ));
    {
        let queued = runtime.acquire(&second);
        tokio::pin!(queued);
        assert!(futures_util::poll!(&mut queued).is_pending());
    }
    let queued = runtime.acquire(&third);
    tokio::pin!(queued);
    assert!(futures_util::poll!(&mut queued).is_pending());
    runtime.set_concurrency(2);
    let third_permit = queued.await.unwrap();
    runtime.set_concurrency(1);
    drop(permit);
    let second_queued = runtime.acquire(&second);
    tokio::pin!(second_queued);
    assert!(futures_util::poll!(&mut second_queued).is_pending());
    drop(third_permit);
    let _second_permit = second_queued.await.unwrap();
}

#[tokio::test]
async fn takeover_rejects_previously_queued_commands_even_after_hand_back() {
    let runtime = AgentRuntime::new(1);
    let parent = group(&runtime);
    let first = child(&runtime, &parent);
    let second = child(&runtime, &parent);
    let resources = AgentResourceCoordinator::default();
    let resource = runtime
        .snapshot(&parent)
        .unwrap()
        .scope
        .targets
        .into_iter()
        .next()
        .unwrap();
    let _lease = resources
        .acquire(
            resource.clone(),
            first.clone(),
            runtime.cancellation(&first).unwrap(),
        )
        .await
        .unwrap();
    let queued = resources.acquire(
        resource.clone(),
        second.clone(),
        runtime.cancellation(&second).unwrap(),
    );
    tokio::pin!(queued);
    assert!(futures_util::poll!(&mut queued).is_pending());
    resources.invalidate(&resource);
    resources.allow_new_requests(&resource);
    assert_eq!(queued.await, Err(AgentError::ResourceUnresolved));
}

#[tokio::test]
async fn cancellation_is_scoped_and_disallows_more_work() {
    let runtime = AgentRuntime::new(2);
    let parent = group(&runtime);
    let first = child(&runtime, &parent);
    let unrelated_parent = runtime.create_group(
        "other-conversation".into(),
        runtime.snapshot(&parent).unwrap().model,
        AgentScope::default(),
        2,
    );
    assert_eq!(
        runtime.complete(&parent, result()),
        Err(AgentError::InvalidState)
    );
    runtime.cancel_group(&parent).unwrap();
    assert_eq!(runtime.take_round(&first), Err(AgentError::Cancelled));
    assert_eq!(
        runtime.take_final_summary(&parent),
        Err(AgentError::Cancelled)
    );
    assert_eq!(
        runtime.delegate(
            &parent,
            AgentText::default(),
            AgentText::default(),
            AgentScope::default(),
            None
        ),
        Err(AgentError::Cancelled)
    );
    assert!(runtime.wait_messages(&first).await.is_err());
    runtime.take_round(&unrelated_parent).unwrap();
    assert_eq!(
        runtime.snapshot(&unrelated_parent).unwrap().state,
        AgentState::Running
    );
    runtime.remove_conversation("conversation");
    assert!(runtime.snapshots("conversation").is_empty());
    assert_eq!(runtime.snapshots("other-conversation").len(), 1);
}

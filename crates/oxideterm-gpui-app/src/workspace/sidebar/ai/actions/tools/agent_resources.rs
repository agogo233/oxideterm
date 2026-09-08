pub(in crate::workspace) async fn execute_ai_tool(
    services: &AiModelBackendServices,
    ui_tx: &AiStreamDeliverySender,
    generation: u64,
    tool_session_id: &ToolSessionId,
    conversation_id: &str,
    assistant_id: &str,
    tool_call_id: String,
    tool_name: String,
    args: serde_json::Value,
    post_user_approval: bool,
    dangerous_command_approved: bool,
    mut execution: Option<&mut AgentExecution>,
) -> AiExecutedToolResult {
    let mut leases = Vec::new();
    if let Some(agent) = execution.as_mut() {
        if !agent
            .runtime
            .snapshot(&agent.run)
            .is_ok_and(|snapshot| snapshot.scope.tools.contains(&tool_name))
        {
            return rejected_ai_tool_result(
                tool_call_id,
                tool_name,
                "agent_scope_denied",
                "This tool is outside the delegated scope.",
            );
        }
        let (sender, receiver) = tokio::sync::oneshot::channel();
        if send_ai_stream_delivery(
            ui_tx,
            generation,
            conversation_id,
            assistant_id,
            AiStreamDeliveryEvent::ToolResourcesRequested {
                tool_session_id: tool_session_id.clone(),
                name: tool_name.clone(),
                args: args.clone(),
                sender,
            },
        )
        .is_err()
        {
            return rejected_ai_tool_result(
                tool_call_id,
                tool_name,
                "operation_cancelled",
                "Tool host is unavailable.",
            );
        }
        let keys = match receiver.await {
            Ok(Ok(keys)) => keys,
            _ => {
                return rejected_ai_tool_result(
                    tool_call_id,
                    tool_name,
                    "agent_scope_denied",
                    "The target is unavailable or outside this run's scope.",
                );
            }
        };
        for key in keys {
            let resources = agent.resources.clone();
            if tool_name == "send_terminal_input" {
                if let Some(lease) = resources.owned_by(&key, &agent.run) {
                    leases.push(oxideterm_ai::agent::AgentToolLease::borrow_command(
                        resources, lease,
                    ));
                    continue;
                }
            }
            let cancellation = match agent.runtime.cancellation(&agent.run) {
                Ok(value) => value,
                Err(_) => {
                    return rejected_ai_tool_result(
                        tool_call_id,
                        tool_name,
                        "operation_cancelled",
                        "Agent run was cancelled.",
                    );
                }
            };
            let acquire = resources.acquire(key, agent.run.clone(), cancellation);
            match agent.wait(AgentState::AwaitingResource, acquire).await {
                Ok(Ok(lease)) => leases.push(oxideterm_ai::agent::AgentToolLease::new(
                    resources.clone(),
                    lease,
                )),
                _ => {
                    return rejected_ai_tool_result(
                        tool_call_id,
                        tool_name,
                        "resource_execution_unresolved",
                        "Terminal control was taken over or a previous operation is unresolved. Ask the user to return control explicitly before retrying.",
                    );
                }
            }
        }
    }
    let _response = oxideterm_ai::agent::AgentToolResponse(leases.clone());
    if !ai_tool_requires_ui_thread(&tool_name, &args) {
        for lease in &leases {
            lease.dispatched();
        }
    }
    let result = execute_ai_tool_uncoordinated(
        services,
        ui_tx,
        generation,
        tool_session_id,
        conversation_id,
        assistant_id,
        tool_call_id,
        tool_name,
        args,
        post_user_approval,
        dangerous_command_approved,
        leases.clone(),
    )
    .await;
    for lease in leases {
        lease.finish_response(result.success);
    }
    result
}

impl WorkspaceApp {
    fn ai_tool_resource_keys(
        &self,
        generation: u64,
        session: &ToolSessionId,
        name: &str,
        args: &serde_json::Value,
        cx: &App,
    ) -> Result<Vec<oxideterm_ai::RuntimeOwnerKey>, String> {
        if !self.ai_entity.read(cx).run_accepts_tools(generation)
            || !self
                .ai_runtime_context
                .read(cx)
                .is_active_tool_session(generation, session)
        {
            return Err("Run is inactive".into());
        }
        if !matches!(name, "run_command" | "send_terminal_input")
            && oxideterm_ai::orchestrator_risk_for_tool(name, Some(args))
                == oxideterm_ai::AiActionRisk::Read
        {
            return Ok(Vec::new());
        }
        let mut handles = Vec::new();
        fn collect(value: &serde_json::Value, handles: &mut Vec<String>) {
            if let Some(array) = value.as_array() {
                for value in array {
                    collect(value, handles);
                }
            }
            if let Some(object) = value.as_object() {
                for (key, value) in object {
                    if key.ends_with("handle_id") {
                        if let Some(value) = value.as_str() {
                            handles.push(value.to_owned());
                        }
                    } else if value.is_object() || value.is_array() {
                        collect(value, handles);
                    }
                }
            }
        }
        collect(args, &mut handles);
        let mut keys = handles
            .iter()
            .map(|handle| {
                self.ai_runtime_context
                    .read(cx)
                    .delegated_agent_target(session, handle)
                    .map(|(key, _)| key)
                    .map_err(|_| "Runtime target expired".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        if keys.is_empty() {
            keys.push(
                self.ai_entity
                    .read(cx)
                    .agents
                    .services
                    .resources
                    .workspace_resource(),
            );
        }
        // Multiple-resource mutations use one deterministic acquisition order.
        keys.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        keys.dedup();
        Ok(keys)
    }

    fn monitor_ai_terminal_command(
        &mut self,
        pane: gpui::Entity<TerminalPane>,
        command_id: Option<String>,
        leases: Vec<oxideterm_ai::agent::AgentToolLease>,
        cx: &mut Context<Self>,
    ) {
        for lease in leases {
            lease.monitor_command();
            let key = lease.lease().resource.clone();
            let pane = pane.downgrade();
            let command_id = command_id.clone();
            let resources = self.ai_entity.read(cx).agents.services.resources.clone();
            let monitor_key = key.clone();
            let task = cx.spawn(async move |weak, cx| {
                loop {
                    Timer::after(Duration::from_millis(250)).await;
                    let finished = weak.update(cx, |_, cx| {
                        if !lease.is_current() { return true; }
                        let Some(pane) = pane.upgrade() else { resources.invalidate(&monitor_key); return true; };
                        let pane = pane.read(cx);
                        if !pane.ai_accepts_input() { resources.invalidate(&monitor_key); return true; }
                        if lease.response_finished() && command_id.as_deref().and_then(|id| pane.ai_command_status(id)).is_some_and(|status| status == oxideterm_gpui_terminal::TerminalCommandFactStatus::Closed) {
                            lease.command_finished();
                            return true;
                        }
                        false
                    });
                    if finished.unwrap_or(true) { break; }
                }
                let _ = weak.update(cx, |this, cx| { this.ai_entity.update(cx, |ai, _cx| { ai.agents.command_monitors.remove(&monitor_key); }); });
            });
            self.ai_entity.update(cx, |ai, _cx| {
                ai.agents.command_monitors.insert(key, task);
            });
        }
    }
}

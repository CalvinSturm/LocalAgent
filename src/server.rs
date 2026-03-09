use std::io::BufRead;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::routing::get;
use axum::{Json, Router};
use clap::Parser;
use futures_util::stream;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch, Notify};

use crate::cli_args::{AttachArgs, ServeArgs};
use crate::operator_queue::{QueueMessageKind, QueueSubmitRequest};
use crate::store::StatePaths;
use crate::{
    provider_runtime, AgentExitReason, Cli, MockProvider, OllamaProvider, OpenAiCompatProvider,
    ProviderKind,
};

#[derive(Debug)]
struct BackendState {
    backend_instance_id: String,
    started_at: String,
    bind: SocketAddr,
    state_dir: String,
    workdir: PathBuf,
    paths: StatePaths,
    sessions: Mutex<Vec<SessionRecord>>,
    runs: Mutex<Vec<RunRecord>>,
}

#[derive(Debug, Serialize)]
struct ServerInfoV1 {
    schema_version: &'static str,
    backend_instance_id: String,
    status: &'static str,
    started_at: String,
    state_dir: String,
    transport: &'static str,
    bind: String,
}

#[derive(Debug, Serialize)]
struct ServerCapabilitiesV1 {
    schema_version: &'static str,
    attach: bool,
    session_registry: bool,
    event_stream: bool,
}

#[derive(Debug, Clone, Serialize)]
struct SessionRecord {
    session_id: String,
    session_name: String,
    status: &'static str,
    active_run_id: Option<String>,
    last_run_id: Option<String>,
    state_mode: &'static str,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct SessionListV1 {
    schema_version: &'static str,
    backend_instance_id: String,
    sessions: Vec<SessionSummaryV1>,
}

#[derive(Debug, Serialize)]
struct SessionSummaryV1 {
    session_id: String,
    session_name: String,
    status: &'static str,
    active_run_id: Option<String>,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct SessionInfoV1 {
    schema_version: &'static str,
    backend_instance_id: String,
    session_id: String,
    session_name: String,
    status: &'static str,
    active_run_id: Option<String>,
    last_run_id: Option<String>,
    state_mode: &'static str,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct CreateSessionRequestV1 {
    session_name: String,
}

#[derive(Debug, Serialize)]
struct SessionCreatedV1 {
    schema_version: &'static str,
    backend_instance_id: String,
    session_id: String,
    session_name: String,
    status: &'static str,
}

#[derive(Debug, Clone)]
struct RunRecord {
    run_id: String,
    session_id: String,
    runtime_run_id: Option<String>,
    prompt: String,
    provider: String,
    model: String,
    status: String,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    exit_reason: Option<String>,
    error: Option<String>,
    run_artifact_path: Option<String>,
    projected_events: Vec<crate::events::ProjectedRunEventV1>,
    event_notify: Arc<Notify>,
    input_tx: Option<std::sync::mpsc::Sender<QueueSubmitRequest>>,
    cancel_tx: Option<watch::Sender<bool>>,
}

#[derive(Debug, Deserialize)]
struct CreateRunRequestV1 {
    prompt: String,
    provider: String,
    model: String,
}

#[derive(Debug, Serialize)]
struct RunCreatedV1 {
    schema_version: &'static str,
    backend_instance_id: String,
    session_id: String,
    run_id: String,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct RunInfoV1 {
    schema_version: &'static str,
    backend_instance_id: String,
    run_id: String,
    runtime_run_id: Option<String>,
    session_id: String,
    status: String,
    prompt: String,
    provider: String,
    model: String,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    exit_reason: Option<String>,
    error: Option<String>,
    run_artifact_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct AttachSessionV1 {
    schema_version: &'static str,
    backend_instance_id: String,
    session_id: String,
    session_name: String,
    active_run_id: Option<String>,
    last_run_id: Option<String>,
    next_event_sequence: u64,
}

#[derive(Debug, Deserialize)]
struct RunEventsQueryV1 {
    after: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SubmitRunInputRequestV1 {
    kind: QueueMessageKind,
    content: String,
}

#[derive(Debug, Deserialize)]
struct RunControlRequestV1 {
    content: String,
}

#[derive(Debug, Serialize)]
struct SubmitRunInputAcceptedV1 {
    schema_version: &'static str,
    backend_instance_id: String,
    run_id: String,
    status: &'static str,
    kind: QueueMessageKind,
}

#[derive(Debug, Serialize)]
struct CancelRunAcceptedV1 {
    schema_version: &'static str,
    backend_instance_id: String,
    run_id: String,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct RunEventsV1 {
    schema_version: &'static str,
    backend_instance_id: String,
    run_id: String,
    events: Vec<crate::events::ProjectedRunEventV1>,
    next_sequence: u64,
}

#[derive(Debug, Serialize)]
struct ErrorEnvelopeV1 {
    schema_version: &'static str,
    error: ErrorBodyV1,
}

#[derive(Debug, Serialize)]
struct ErrorBodyV1 {
    code: &'static str,
    message: String,
    backend_instance_id: String,
}

struct LiveRunEventStreamState {
    state: Arc<BackendState>,
    run_id: String,
    next_sequence: u64,
    notify: Arc<Notify>,
}

enum LiveRunEventDecision {
    Emit {
        event: crate::events::ProjectedRunEventV1,
        notify: Arc<Notify>,
    },
    Wait(Arc<Notify>),
    Done,
}

pub(crate) async fn run_server(args: &ServeArgs, paths: &StatePaths) -> anyhow::Result<()> {
    let bind_ip: Ipv4Addr = args
        .bind
        .parse()
        .with_context(|| format!("invalid --bind IPv4 address: {}", args.bind))?;
    if !bind_ip.is_loopback() {
        return Err(anyhow!(
            "--bind must be a loopback IPv4 address for Phase 2 PR1"
        ));
    }

    let socket = SocketAddr::V4(SocketAddrV4::new(bind_ip, args.port));
    let listener = tokio::net::TcpListener::bind(socket)
        .await
        .with_context(|| format!("failed binding serve listener on {socket}"))?;
    let local_addr = listener
        .local_addr()
        .context("failed reading serve listener local address")?;

    let state = Arc::new(BackendState {
        backend_instance_id: format!("b_{}", ulid::Ulid::new()),
        started_at: crate::trust::now_rfc3339(),
        bind: local_addr,
        state_dir: paths.state_dir.display().to_string(),
        workdir: std::env::current_dir()
            .context("failed to resolve current directory for serve")?,
        paths: paths.clone(),
        sessions: Mutex::new(Vec::new()),
        runs: Mutex::new(Vec::new()),
    });
    let app = build_router(state.clone());

    println!(
        "{}",
        serde_json::to_string_pretty(&ServerInfoV1::from_state(&state))?
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .context("serve loop failed")?;

    Ok(())
}

pub(crate) async fn run_attach_client(args: &AttachArgs) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let base = args.server_url.trim_end_matches('/');
    let attach_url = format!("{base}/v1/sessions/{}/attach", args.session_id);
    let attach_payload: serde_json::Value = client
        .post(&attach_url)
        .send()
        .await
        .with_context(|| format!("attach request failed: {attach_url}"))?
        .error_for_status()
        .context("attach request returned error status")?
        .json()
        .await
        .context("failed to parse attach response")?;
    let run_id = attach_payload["active_run_id"]
        .as_str()
        .or_else(|| attach_payload["last_run_id"].as_str())
        .ok_or_else(|| anyhow!("attach did not return an active_run_id or last_run_id"))?;
    let interactive = attach_payload["active_run_id"].as_str().is_some();
    let after = attach_payload["next_event_sequence"]
        .as_u64()
        .unwrap_or(1)
        .saturating_sub(1);
    let stream_url = format!("{base}/v1/runs/{run_id}/events/stream?after={after}");
    let response = client
        .get(&stream_url)
        .send()
        .await
        .with_context(|| format!("event stream request failed: {stream_url}"))?
        .error_for_status()
        .context("event stream returned error status")?;
    let mut byte_stream = response.bytes_stream();
    let mut command_rx = start_attach_command_reader(interactive);
    if interactive {
        print_attach_help(run_id);
    } else {
        eprintln!("attach: no active run; streaming existing run {run_id} in read-only mode");
    }
    let mut buffer = String::new();
    loop {
        tokio::select! {
            chunk = byte_stream.next() => {
                let Some(chunk) = chunk else {
                    break;
                };
                let chunk = chunk.context("failed reading SSE stream chunk")?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));
                while let Some(split_idx) = buffer.find("\n\n") {
                    let frame = buffer[..split_idx].to_string();
                    buffer = buffer[split_idx + 2..].to_string();
                    if let Some(data) = parse_sse_data_frame(&frame) {
                        println!("{data}");
                    }
                }
            }
            maybe_cmd = command_rx.recv(), if interactive => {
                let Some(cmd) = maybe_cmd else {
                    continue;
                };
                if !dispatch_attach_command(&client, base, run_id, &cmd).await? {
                    break;
                }
            }
        }
    }
    if !buffer.trim().is_empty() {
        if let Some(data) = parse_sse_data_frame(&buffer) {
            println!("{data}");
        }
    }
    Ok(())
}

fn build_router(state: Arc<BackendState>) -> Router {
    Router::new()
        .route("/v1/server", get(get_server_info))
        .route("/v1/server/capabilities", get(get_server_capabilities))
        .route("/v1/sessions", get(list_sessions).post(create_session))
        .route("/v1/sessions/{session_id}", get(get_session))
        .route(
            "/v1/sessions/{session_id}/attach",
            axum::routing::post(attach_session),
        )
        .route(
            "/v1/sessions/{session_id}/runs",
            axum::routing::post(create_run),
        )
        .route("/v1/runs/{run_id}", get(get_run))
        .route("/v1/runs/{run_id}/events", get(list_run_events))
        .route("/v1/runs/{run_id}/events/stream", get(stream_run_events))
        .route(
            "/v1/runs/{run_id}/input",
            axum::routing::post(submit_run_input),
        )
        .route(
            "/v1/runs/{run_id}/interrupt",
            axum::routing::post(interrupt_run),
        )
        .route(
            "/v1/runs/{run_id}/next",
            axum::routing::post(submit_follow_up_run),
        )
        .route("/v1/runs/{run_id}/cancel", axum::routing::post(cancel_run))
        .with_state(state)
}

async fn get_server_info(State(state): State<Arc<BackendState>>) -> Json<ServerInfoV1> {
    Json(ServerInfoV1::from_state(&state))
}

async fn get_server_capabilities(
    State(_state): State<Arc<BackendState>>,
) -> Json<ServerCapabilitiesV1> {
    Json(ServerCapabilitiesV1 {
        schema_version: "v1",
        attach: true,
        session_registry: true,
        event_stream: true,
    })
}

async fn list_sessions(State(state): State<Arc<BackendState>>) -> Json<SessionListV1> {
    let sessions = state
        .sessions
        .lock()
        .expect("session registry lock")
        .iter()
        .map(SessionSummaryV1::from_record)
        .collect();
    Json(SessionListV1 {
        schema_version: "v1",
        backend_instance_id: state.backend_instance_id.clone(),
        sessions,
    })
}

async fn get_session(
    Path(session_id): Path<String>,
    State(state): State<Arc<BackendState>>,
) -> Result<Json<SessionInfoV1>, (StatusCode, Json<ErrorEnvelopeV1>)> {
    let sessions = state.sessions.lock().expect("session registry lock");
    let Some(record) = sessions
        .iter()
        .find(|record| record.session_id == session_id)
    else {
        return Err(not_found_error(&state, "session not found"));
    };
    Ok(Json(SessionInfoV1::from_record(
        &state.backend_instance_id,
        record,
    )))
}

async fn create_session(
    State(state): State<Arc<BackendState>>,
    Json(req): Json<CreateSessionRequestV1>,
) -> Result<(StatusCode, Json<SessionCreatedV1>), (StatusCode, Json<ErrorEnvelopeV1>)> {
    let session_name = req.session_name.trim();
    if session_name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorEnvelopeV1 {
                schema_version: "v1",
                error: ErrorBodyV1 {
                    code: "INVALID_SESSION_NAME",
                    message: "session_name must not be empty".to_string(),
                    backend_instance_id: state.backend_instance_id.clone(),
                },
            }),
        ));
    }

    let record = SessionRecord {
        session_id: format!("s_{}", ulid::Ulid::new()),
        session_name: session_name.to_string(),
        status: "idle",
        active_run_id: None,
        last_run_id: None,
        state_mode: "persistent",
        updated_at: crate::trust::now_rfc3339(),
    };

    let mut sessions = state.sessions.lock().expect("session registry lock");
    sessions.push(record.clone());

    Ok((
        StatusCode::CREATED,
        Json(SessionCreatedV1 {
            schema_version: "v1",
            backend_instance_id: state.backend_instance_id.clone(),
            session_id: record.session_id,
            session_name: record.session_name,
            status: record.status,
        }),
    ))
}

async fn create_run(
    Path(session_id): Path<String>,
    State(state): State<Arc<BackendState>>,
    Json(req): Json<CreateRunRequestV1>,
) -> Result<(StatusCode, Json<RunCreatedV1>), (StatusCode, Json<ErrorEnvelopeV1>)> {
    let prompt = req.prompt.trim();
    let provider = req.provider.trim();
    let model = req.model.trim();
    if prompt.is_empty() || provider.is_empty() || model.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorEnvelopeV1 {
                schema_version: "v1",
                error: ErrorBodyV1 {
                    code: "INVALID_RUN_REQUEST",
                    message: "prompt, provider, and model must not be empty".to_string(),
                    backend_instance_id: state.backend_instance_id.clone(),
                },
            }),
        ));
    }

    let run_id = format!("r_{}", ulid::Ulid::new());
    {
        let mut sessions = state.sessions.lock().expect("session registry lock");
        let Some(record) = sessions
            .iter_mut()
            .find(|record| record.session_id == session_id)
        else {
            return Err(not_found_error(&state, "session not found"));
        };
        record.active_run_id = Some(run_id.clone());
        record.last_run_id = Some(run_id.clone());
        record.updated_at = crate::trust::now_rfc3339();
    }

    let (input_tx, input_rx) = std::sync::mpsc::channel::<QueueSubmitRequest>();
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let event_notify = Arc::new(Notify::new());
    state
        .runs
        .lock()
        .expect("run registry lock")
        .push(RunRecord {
            run_id: run_id.clone(),
            session_id: session_id.clone(),
            runtime_run_id: None,
            prompt: prompt.to_string(),
            provider: provider.to_string(),
            model: model.to_string(),
            status: "created".to_string(),
            created_at: crate::trust::now_rfc3339(),
            started_at: None,
            finished_at: None,
            exit_reason: None,
            error: None,
            run_artifact_path: None,
            projected_events: Vec::new(),
            event_notify: event_notify.clone(),
            input_tx: Some(input_tx.clone()),
            cancel_tx: Some(cancel_tx.clone()),
        });

    let state_for_task = state.clone();
    let prompt_for_task = prompt.to_string();
    let provider_for_task = provider.to_string();
    let model_for_task = model.to_string();
    let session_id_for_task = session_id.clone();
    let run_id_for_task = run_id.clone();
    tokio::spawn(async move {
        execute_backend_run(
            state_for_task,
            session_id_for_task,
            run_id_for_task,
            prompt_for_task,
            provider_for_task,
            model_for_task,
            input_rx,
            (cancel_tx, cancel_rx),
        )
        .await;
    });

    Ok((
        StatusCode::CREATED,
        Json(RunCreatedV1 {
            schema_version: "v1",
            backend_instance_id: state.backend_instance_id.clone(),
            session_id,
            run_id,
            status: "running",
        }),
    ))
}

async fn get_run(
    Path(run_id): Path<String>,
    State(state): State<Arc<BackendState>>,
) -> Result<Json<RunInfoV1>, (StatusCode, Json<ErrorEnvelopeV1>)> {
    let runs = state.runs.lock().expect("run registry lock");
    let Some(record) = runs.iter().find(|record| record.run_id == run_id) else {
        return Err(run_not_found_error(&state, "run not found"));
    };
    Ok(Json(RunInfoV1::from_record(
        &state.backend_instance_id,
        record,
    )))
}

async fn attach_session(
    Path(session_id): Path<String>,
    State(state): State<Arc<BackendState>>,
) -> Result<Json<AttachSessionV1>, (StatusCode, Json<ErrorEnvelopeV1>)> {
    let sessions = state.sessions.lock().expect("session registry lock");
    let Some(record) = sessions
        .iter()
        .find(|record| record.session_id == session_id)
    else {
        return Err(not_found_error(&state, "session not found"));
    };
    let next_event_sequence = if let Some(active_run_id) = &record.active_run_id {
        state
            .runs
            .lock()
            .expect("run registry lock")
            .iter()
            .find(|run| &run.run_id == active_run_id)
            .map(|run| run.projected_events.len() as u64 + 1)
            .unwrap_or(1)
    } else {
        1
    };
    Ok(Json(AttachSessionV1 {
        schema_version: "v1",
        backend_instance_id: state.backend_instance_id.clone(),
        session_id: record.session_id.clone(),
        session_name: record.session_name.clone(),
        active_run_id: record.active_run_id.clone(),
        last_run_id: record.last_run_id.clone(),
        next_event_sequence,
    }))
}

async fn list_run_events(
    Path(run_id): Path<String>,
    Query(query): Query<RunEventsQueryV1>,
    State(state): State<Arc<BackendState>>,
) -> Result<Json<RunEventsV1>, (StatusCode, Json<ErrorEnvelopeV1>)> {
    let runs = state.runs.lock().expect("run registry lock");
    let Some(record) = runs.iter().find(|record| record.run_id == run_id) else {
        return Err(run_not_found_error(&state, "run not found"));
    };
    let after = query.after.unwrap_or(0);
    let events = record
        .projected_events
        .iter()
        .filter(|event| event.sequence > after)
        .cloned()
        .collect::<Vec<_>>();
    let next_sequence = record.projected_events.len() as u64 + 1;
    Ok(Json(RunEventsV1 {
        schema_version: "v1",
        backend_instance_id: state.backend_instance_id.clone(),
        run_id: record.run_id.clone(),
        events,
        next_sequence,
    }))
}

async fn stream_run_events(
    Path(run_id): Path<String>,
    Query(query): Query<RunEventsQueryV1>,
    State(state): State<Arc<BackendState>>,
) -> Result<
    Sse<impl futures_util::Stream<Item = Result<SseEvent, std::convert::Infallible>>>,
    (StatusCode, Json<ErrorEnvelopeV1>),
> {
    let after = query.after.unwrap_or(0);
    let (initial_cursor, initial_notify) = {
        let runs = state.runs.lock().expect("run registry lock");
        let Some(record) = runs.iter().find(|record| record.run_id == run_id) else {
            return Err(run_not_found_error(&state, "run not found"));
        };
        (after.saturating_add(1), record.event_notify.clone())
    };
    let stream_state = LiveRunEventStreamState {
        state,
        run_id,
        next_sequence: initial_cursor,
        notify: initial_notify,
    };
    let event_stream = stream::unfold(stream_state, |mut stream_state| async move {
        loop {
            let decision = {
                let runs = stream_state.state.runs.lock().expect("run registry lock");
                let Some(record) = runs
                    .iter()
                    .find(|record| record.run_id == stream_state.run_id)
                else {
                    return None;
                };
                if let Some(event) = record
                    .projected_events
                    .iter()
                    .find(|event| event.sequence >= stream_state.next_sequence)
                    .cloned()
                {
                    LiveRunEventDecision::Emit {
                        event,
                        notify: record.event_notify.clone(),
                    }
                } else if is_terminal_run_status(&record.status) {
                    LiveRunEventDecision::Done
                } else {
                    LiveRunEventDecision::Wait(record.event_notify.clone())
                }
            };
            match decision {
                LiveRunEventDecision::Emit { event, notify } => {
                    stream_state.next_sequence = event.sequence.saturating_add(1);
                    stream_state.notify = notify;
                    let data = serde_json::to_string(&event).expect("projected event json");
                    return Some((
                        Ok(SseEvent::default()
                            .event("run_event")
                            .id(event.sequence.to_string())
                            .data(data)),
                        stream_state,
                    ));
                }
                LiveRunEventDecision::Done => return None,
                LiveRunEventDecision::Wait(notify) => {
                    stream_state.notify = notify.clone();
                    tokio::select! {
                        _ = notify.notified() => {}
                        _ = tokio::time::sleep(Duration::from_secs(10)) => {
                            return Some((
                                Ok(SseEvent::default().event("keepalive").data("{}")),
                                stream_state,
                            ));
                        }
                    }
                }
            }
        }
    });
    Ok(Sse::new(event_stream).keep_alive(KeepAlive::default()))
}

async fn submit_run_input(
    Path(run_id): Path<String>,
    State(state): State<Arc<BackendState>>,
    Json(req): Json<SubmitRunInputRequestV1>,
) -> Result<(StatusCode, Json<SubmitRunInputAcceptedV1>), (StatusCode, Json<ErrorEnvelopeV1>)> {
    submit_run_input_inner(&state, run_id, req.kind, req.content).await
}

async fn interrupt_run(
    Path(run_id): Path<String>,
    State(state): State<Arc<BackendState>>,
    Json(req): Json<RunControlRequestV1>,
) -> Result<(StatusCode, Json<SubmitRunInputAcceptedV1>), (StatusCode, Json<ErrorEnvelopeV1>)> {
    submit_run_input_inner(&state, run_id, QueueMessageKind::Steer, req.content).await
}

async fn submit_follow_up_run(
    Path(run_id): Path<String>,
    State(state): State<Arc<BackendState>>,
    Json(req): Json<RunControlRequestV1>,
) -> Result<(StatusCode, Json<SubmitRunInputAcceptedV1>), (StatusCode, Json<ErrorEnvelopeV1>)> {
    submit_run_input_inner(&state, run_id, QueueMessageKind::FollowUp, req.content).await
}

async fn submit_run_input_inner(
    state: &Arc<BackendState>,
    run_id: String,
    kind: QueueMessageKind,
    content: String,
) -> Result<(StatusCode, Json<SubmitRunInputAcceptedV1>), (StatusCode, Json<ErrorEnvelopeV1>)> {
    let content = content.trim();
    if content.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorEnvelopeV1 {
                schema_version: "v1",
                error: ErrorBodyV1 {
                    code: "INVALID_RUN_INPUT",
                    message: "content must not be empty".to_string(),
                    backend_instance_id: state.backend_instance_id.clone(),
                },
            }),
        ));
    }
    let input_tx = {
        let runs = state.runs.lock().expect("run registry lock");
        let Some(record) = runs.iter().find(|record| record.run_id == run_id) else {
            return Err(run_not_found_error(&state, "run not found"));
        };
        if !matches!(record.status.as_str(), "created" | "running") {
            return Err(run_not_active_error(&state, "run is not active"));
        }
        record
            .input_tx
            .clone()
            .ok_or_else(|| run_not_active_error(&state, "run input is unavailable"))?
    };
    input_tx
        .send(QueueSubmitRequest {
            kind,
            content: content.to_string(),
        })
        .map_err(|_| run_not_active_error(&state, "run input channel is closed"))?;
    Ok((
        StatusCode::ACCEPTED,
        Json(SubmitRunInputAcceptedV1 {
            schema_version: "v1",
            backend_instance_id: state.backend_instance_id.clone(),
            run_id,
            status: "accepted",
            kind,
        }),
    ))
}

async fn cancel_run(
    Path(run_id): Path<String>,
    State(state): State<Arc<BackendState>>,
) -> Result<(StatusCode, Json<CancelRunAcceptedV1>), (StatusCode, Json<ErrorEnvelopeV1>)> {
    let cancel_tx = {
        let runs = state.runs.lock().expect("run registry lock");
        let Some(record) = runs.iter().find(|record| record.run_id == run_id) else {
            return Err(run_not_found_error(&state, "run not found"));
        };
        if !matches!(record.status.as_str(), "created" | "running") {
            return Err(run_not_active_error(&state, "run is not active"));
        }
        record
            .cancel_tx
            .clone()
            .ok_or_else(|| run_not_active_error(&state, "run cancel is unavailable"))?
    };
    cancel_tx
        .send(true)
        .map_err(|_| run_not_active_error(&state, "run cancel channel is closed"))?;
    Ok((
        StatusCode::ACCEPTED,
        Json(CancelRunAcceptedV1 {
            schema_version: "v1",
            backend_instance_id: state.backend_instance_id.clone(),
            run_id,
            status: "cancel_requested",
        }),
    ))
}

impl ServerInfoV1 {
    fn from_state(state: &BackendState) -> Self {
        Self {
            schema_version: "v1",
            backend_instance_id: state.backend_instance_id.clone(),
            status: "ready",
            started_at: state.started_at.clone(),
            state_dir: state.state_dir.clone(),
            transport: "http",
            bind: state.bind.to_string(),
        }
    }
}

impl SessionSummaryV1 {
    fn from_record(record: &SessionRecord) -> Self {
        Self {
            session_id: record.session_id.clone(),
            session_name: record.session_name.clone(),
            status: record.status,
            active_run_id: record.active_run_id.clone(),
            updated_at: record.updated_at.clone(),
        }
    }
}

impl SessionInfoV1 {
    fn from_record(backend_instance_id: &str, record: &SessionRecord) -> Self {
        Self {
            schema_version: "v1",
            backend_instance_id: backend_instance_id.to_string(),
            session_id: record.session_id.clone(),
            session_name: record.session_name.clone(),
            status: record.status,
            active_run_id: record.active_run_id.clone(),
            last_run_id: record.last_run_id.clone(),
            state_mode: record.state_mode,
            updated_at: record.updated_at.clone(),
        }
    }
}

impl RunInfoV1 {
    fn from_record(backend_instance_id: &str, record: &RunRecord) -> Self {
        Self {
            schema_version: "v1",
            backend_instance_id: backend_instance_id.to_string(),
            run_id: record.run_id.clone(),
            runtime_run_id: record.runtime_run_id.clone(),
            session_id: record.session_id.clone(),
            status: record.status.clone(),
            prompt: record.prompt.clone(),
            provider: record.provider.clone(),
            model: record.model.clone(),
            created_at: record.created_at.clone(),
            started_at: record.started_at.clone(),
            finished_at: record.finished_at.clone(),
            exit_reason: record.exit_reason.clone(),
            error: record.error.clone(),
            run_artifact_path: record.run_artifact_path.clone(),
        }
    }
}

fn not_found_error(state: &BackendState, message: &str) -> (StatusCode, Json<ErrorEnvelopeV1>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorEnvelopeV1 {
            schema_version: "v1",
            error: ErrorBodyV1 {
                code: "SESSION_NOT_FOUND",
                message: message.to_string(),
                backend_instance_id: state.backend_instance_id.clone(),
            },
        }),
    )
}

fn run_not_found_error(state: &BackendState, message: &str) -> (StatusCode, Json<ErrorEnvelopeV1>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorEnvelopeV1 {
            schema_version: "v1",
            error: ErrorBodyV1 {
                code: "RUN_NOT_FOUND",
                message: message.to_string(),
                backend_instance_id: state.backend_instance_id.clone(),
            },
        }),
    )
}

fn run_not_active_error(
    state: &BackendState,
    message: &str,
) -> (StatusCode, Json<ErrorEnvelopeV1>) {
    (
        StatusCode::CONFLICT,
        Json(ErrorEnvelopeV1 {
            schema_version: "v1",
            error: ErrorBodyV1 {
                code: "RUN_NOT_ACTIVE",
                message: message.to_string(),
                backend_instance_id: state.backend_instance_id.clone(),
            },
        }),
    )
}

async fn execute_backend_run(
    state: Arc<BackendState>,
    session_id: String,
    run_id: String,
    prompt: String,
    provider: String,
    model: String,
    operator_queue_rx: std::sync::mpsc::Receiver<QueueSubmitRequest>,
    external_cancel_pair: (watch::Sender<bool>, watch::Receiver<bool>),
) {
    update_run_record(&state, &run_id, |record| {
        record.status = "running".to_string();
        record.started_at = Some(crate::trust::now_rfc3339());
        record.event_notify.notify_waiters();
    });

    let result = execute_backend_run_inner(
        state.clone(),
        &run_id,
        &session_id,
        &prompt,
        &provider,
        &model,
        operator_queue_rx,
        external_cancel_pair,
    )
    .await;
    match result {
        Ok(exec) => {
            let exit_reason = exec.outcome.exit_reason.as_str().to_string();
            let terminal_status =
                terminal_status_for_exit_reason(exec.outcome.exit_reason).to_string();
            let error = exec.outcome.error.clone();
            let finished_at = Some(exec.outcome.finished_at.clone());
            let run_artifact_path = exec
                .run_artifact_path
                .as_ref()
                .map(|path| path.display().to_string());
            update_run_record(&state, &run_id, |record| {
                record.status = terminal_status.clone();
                record.finished_at = finished_at.clone();
                record.runtime_run_id = Some(exec.outcome.run_id.clone());
                record.exit_reason = Some(exit_reason.clone());
                record.error = error.clone();
                record.run_artifact_path = run_artifact_path.clone();
                record.input_tx = None;
                record.cancel_tx = None;
                record.event_notify.notify_waiters();
            });
        }
        Err(err) => {
            update_run_record(&state, &run_id, |record| {
                record.status = "failed".to_string();
                record.finished_at = Some(crate::trust::now_rfc3339());
                record.exit_reason = Some(AgentExitReason::ProviderError.as_str().to_string());
                record.error = Some(err.to_string());
                record.input_tx = None;
                record.cancel_tx = None;
                record.event_notify.notify_waiters();
            });
        }
    }

    let mut sessions = state.sessions.lock().expect("session registry lock");
    if let Some(record) = sessions
        .iter_mut()
        .find(|record| record.session_id == session_id)
    {
        record.active_run_id = None;
        record.last_run_id = Some(run_id);
        record.updated_at = crate::trust::now_rfc3339();
    }
}

async fn execute_backend_run_inner(
    state: Arc<BackendState>,
    run_id: &str,
    session_id: &str,
    prompt: &str,
    provider: &str,
    model: &str,
    operator_queue_rx: std::sync::mpsc::Receiver<QueueSubmitRequest>,
    external_cancel_pair: (watch::Sender<bool>, watch::Receiver<bool>),
) -> anyhow::Result<crate::RunExecutionResult> {
    let provider_kind = parse_provider_kind(provider)?;
    let base_url = provider_runtime::default_base_url(provider_kind).to_string();
    let mut args = Cli::parse_from(["localagent"]).run;
    args.provider = Some(provider_kind);
    args.model = Some(model.to_string());
    args.base_url = Some(base_url.clone());
    args.prompt = Some(prompt.to_string());
    args.workdir = state.workdir.clone();
    args.state_dir = Some(state.paths.state_dir.clone());
    args.session = session_id.to_string();
    args.no_session = false;
    args.stream = false;
    args.tui = false;
    args.disable_implementation_guard = true;
    let (ui_tx, ui_rx) = std::sync::mpsc::channel::<crate::events::Event>();
    let event_state = state.clone();
    let event_run_id = run_id.to_string();
    let event_thread = std::thread::spawn(move || {
        let mut sequence = 0u64;
        while let Ok(event) = ui_rx.recv() {
            sequence = sequence.saturating_add(1);
            if let Some(projected) = crate::events::project_event_v1(&event, sequence) {
                update_run_record(&event_state, &event_run_id, |record| {
                    record.projected_events.push(projected);
                    record.event_notify.notify_waiters();
                });
            }
        }
    });

    let result = match provider_kind {
        ProviderKind::Lmstudio | ProviderKind::Llamacpp => {
            let provider = OpenAiCompatProvider::new(
                provider_kind,
                base_url.clone(),
                args.api_key.clone(),
                provider_runtime::http_config_from_run_args(&args),
            )?;
            crate::run_agent_with_ui(
                provider,
                provider_kind,
                &base_url,
                model,
                prompt,
                &args,
                &state.paths,
                Some(ui_tx),
                Some(operator_queue_rx),
                Some(external_cancel_pair),
                None,
                true,
            )
            .await
        }
        ProviderKind::Ollama => {
            let provider = OllamaProvider::new(
                base_url.clone(),
                provider_runtime::http_config_from_run_args(&args),
            )?;
            crate::run_agent_with_ui(
                provider,
                provider_kind,
                &base_url,
                model,
                prompt,
                &args,
                &state.paths,
                Some(ui_tx),
                Some(operator_queue_rx),
                Some(external_cancel_pair),
                None,
                true,
            )
            .await
        }
        ProviderKind::Mock => {
            let provider = MockProvider::new();
            crate::run_agent_with_ui(
                provider,
                provider_kind,
                &base_url,
                model,
                prompt,
                &args,
                &state.paths,
                Some(ui_tx),
                Some(operator_queue_rx),
                Some(external_cancel_pair),
                None,
                true,
            )
            .await
        }
    };
    let _ = event_thread.join();
    result
}

fn parse_provider_kind(value: &str) -> anyhow::Result<ProviderKind> {
    match value {
        "lmstudio" => Ok(ProviderKind::Lmstudio),
        "llamacpp" => Ok(ProviderKind::Llamacpp),
        "ollama" => Ok(ProviderKind::Ollama),
        "mock" => Ok(ProviderKind::Mock),
        _ => Err(anyhow!("unsupported provider '{value}'")),
    }
}

fn terminal_status_for_exit_reason(exit_reason: AgentExitReason) -> &'static str {
    match exit_reason {
        AgentExitReason::Ok => "finished",
        AgentExitReason::Cancelled => "cancelled",
        _ => "failed",
    }
}

fn is_terminal_run_status(status: &str) -> bool {
    matches!(status, "finished" | "failed" | "cancelled")
}

fn parse_sse_data_frame(frame: &str) -> Option<String> {
    let mut event_name = None;
    let mut data_lines = Vec::new();
    for line in frame.lines() {
        if let Some(rest) = line.strip_prefix("event:") {
            event_name = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
    }
    match event_name.as_deref() {
        Some("run_event") => {
            let joined = data_lines.join("\n");
            if joined.is_empty() {
                None
            } else {
                Some(joined)
            }
        }
        _ => None,
    }
}

fn print_attach_help(run_id: &str) {
    eprintln!("attached to live run {run_id}");
    eprintln!("commands: /interrupt <msg>, /next <msg>, /cancel, /help, /exit");
}

fn start_attach_command_reader(enabled: bool) -> mpsc::UnboundedReceiver<String> {
    let (tx, rx) = mpsc::unbounded_channel();
    if !enabled {
        return rx;
    }
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut locked = stdin.lock();
        let mut line = String::new();
        loop {
            line.clear();
            match locked.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim().to_string();
                    if !trimmed.is_empty() && tx.send(trimmed).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    rx
}

async fn dispatch_attach_command(
    client: &reqwest::Client,
    base: &str,
    run_id: &str,
    cmd: &str,
) -> anyhow::Result<bool> {
    if cmd.eq_ignore_ascii_case("/help") {
        print_attach_help(run_id);
        return Ok(true);
    }
    if cmd.eq_ignore_ascii_case("/exit") {
        return Ok(false);
    }
    if let Some(rest) = cmd.strip_prefix("/interrupt ") {
        post_attach_control(client, &format!("{base}/v1/runs/{run_id}/interrupt"), rest).await?;
        eprintln!("queued interrupt");
        return Ok(true);
    }
    if let Some(rest) = cmd.strip_prefix("/next ") {
        post_attach_control(client, &format!("{base}/v1/runs/{run_id}/next"), rest).await?;
        eprintln!("queued next");
        return Ok(true);
    }
    if cmd.eq_ignore_ascii_case("/cancel") {
        client
            .post(format!("{base}/v1/runs/{run_id}/cancel"))
            .send()
            .await
            .context("cancel request failed")?
            .error_for_status()
            .context("cancel request returned error status")?;
        eprintln!("cancel requested");
        return Ok(true);
    }
    eprintln!("unknown command: {cmd}");
    eprintln!("use /help");
    Ok(true)
}

async fn post_attach_control(
    client: &reqwest::Client,
    url: &str,
    content: &str,
) -> anyhow::Result<()> {
    client
        .post(url)
        .json(&serde_json::json!({ "content": content }))
        .send()
        .await
        .with_context(|| format!("control request failed: {url}"))?
        .error_for_status()
        .context("control request returned error status")?;
    Ok(())
}

fn update_run_record(state: &BackendState, run_id: &str, update: impl FnOnce(&mut RunRecord)) {
    let mut runs = state.runs.lock().expect("run registry lock");
    if let Some(record) = runs.iter_mut().find(|record| record.run_id == run_id) {
        update(record);
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use clap::Parser;
    use tempfile::tempdir;
    use tokio::sync::{watch, Notify};
    use tower::util::ServiceExt;

    use super::{build_router, update_run_record, BackendState, RunRecord, SessionRecord};
    use crate::provider_runtime;
    use crate::providers::{ModelProvider, StreamDelta};
    use crate::types::{GenerateRequest, GenerateResponse, Message, Role};
    use crate::ProviderKind;

    #[derive(Debug, Clone)]
    struct DelayedTestProvider {
        delay_ms: u64,
    }

    #[async_trait]
    impl ModelProvider for DelayedTestProvider {
        async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            Ok(GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some("delayed: ok".to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: Vec::new(),
                usage: None,
            })
        }

        fn supports_streaming(&self) -> bool {
            true
        }

        async fn generate_streaming(
            &self,
            req: GenerateRequest,
            on_delta: &mut (dyn FnMut(StreamDelta) + Send),
        ) -> anyhow::Result<GenerateResponse> {
            let out = self.generate(req).await?;
            if let Some(content) = out.assistant.content.clone() {
                on_delta(StreamDelta::Content(content));
            }
            Ok(out)
        }
    }

    fn sample_state() -> Arc<BackendState> {
        let tmp = tempdir().expect("tempdir");
        let workdir = tmp.keep();
        let paths = crate::resolve_state_paths(&workdir, None, None, None, None);
        Arc::new(BackendState {
            backend_instance_id: "b_test".to_string(),
            started_at: "2026-03-08T00:00:00Z".to_string(),
            bind: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 7070)),
            state_dir: paths.state_dir.display().to_string(),
            workdir,
            paths,
            sessions: Mutex::new(Vec::new()),
            runs: Mutex::new(Vec::new()),
        })
    }

    fn insert_running_run(
        state: &Arc<BackendState>,
    ) -> (
        String,
        std::sync::mpsc::Receiver<crate::operator_queue::QueueSubmitRequest>,
        watch::Receiver<bool>,
    ) {
        let session_id = "s_test".to_string();
        let run_id = "r_test".to_string();
        let (input_tx, input_rx) =
            std::sync::mpsc::channel::<crate::operator_queue::QueueSubmitRequest>();
        let (cancel_tx, cancel_rx) = watch::channel(false);
        state
            .sessions
            .lock()
            .expect("session registry lock")
            .push(SessionRecord {
                session_id: session_id.clone(),
                session_name: "default".to_string(),
                status: "idle",
                active_run_id: Some(run_id.clone()),
                last_run_id: Some(run_id.clone()),
                state_mode: "persistent",
                updated_at: crate::trust::now_rfc3339(),
            });
        state
            .runs
            .lock()
            .expect("run registry lock")
            .push(RunRecord {
                run_id: run_id.clone(),
                session_id,
                prompt: "test".to_string(),
                provider: "mock".to_string(),
                model: "example-model".to_string(),
                runtime_run_id: None,
                status: "running".to_string(),
                created_at: crate::trust::now_rfc3339(),
                started_at: Some(crate::trust::now_rfc3339()),
                finished_at: None,
                exit_reason: None,
                error: None,
                run_artifact_path: None,
                projected_events: vec![crate::events::ProjectedRunEventV1 {
                    schema_version: "v1".to_string(),
                    sequence: 1,
                    ts: crate::trust::now_rfc3339(),
                    run_id: run_id.clone(),
                    step: 0,
                    event_type: "run_started".to_string(),
                    data: serde_json::json!({}),
                }],
                event_notify: Arc::new(Notify::new()),
                input_tx: Some(input_tx),
                cancel_tx: Some(cancel_tx),
            });
        (run_id, input_rx, cancel_rx)
    }

    fn spawn_delayed_server_run(state: &Arc<BackendState>, delay_ms: u64) -> (String, String) {
        let session_id = format!("s_{}", ulid::Ulid::new());
        let run_id = format!("r_{}", ulid::Ulid::new());
        let prompt = "Say hi.".to_string();
        let model = "delayed-test".to_string();
        let base_url = provider_runtime::default_base_url(ProviderKind::Mock).to_string();
        let (input_tx, input_rx) =
            std::sync::mpsc::channel::<crate::operator_queue::QueueSubmitRequest>();
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let event_notify = Arc::new(Notify::new());

        state
            .sessions
            .lock()
            .expect("session registry lock")
            .push(SessionRecord {
                session_id: session_id.clone(),
                session_name: "default".to_string(),
                status: "idle",
                active_run_id: Some(run_id.clone()),
                last_run_id: Some(run_id.clone()),
                state_mode: "persistent",
                updated_at: crate::trust::now_rfc3339(),
            });
        state
            .runs
            .lock()
            .expect("run registry lock")
            .push(RunRecord {
                run_id: run_id.clone(),
                session_id: session_id.clone(),
                prompt: prompt.clone(),
                provider: "mock".to_string(),
                model: model.clone(),
                runtime_run_id: None,
                status: "created".to_string(),
                created_at: crate::trust::now_rfc3339(),
                started_at: None,
                finished_at: None,
                exit_reason: None,
                error: None,
                run_artifact_path: None,
                projected_events: Vec::new(),
                event_notify: event_notify.clone(),
                input_tx: Some(input_tx),
                cancel_tx: Some(cancel_tx.clone()),
            });

        let state_for_task = state.clone();
        let run_id_for_task = run_id.clone();
        let session_id_for_task = session_id.clone();
        tokio::spawn(async move {
            update_run_record(&state_for_task, &run_id_for_task, |record| {
                record.status = "running".to_string();
                record.started_at = Some(crate::trust::now_rfc3339());
                record.event_notify.notify_waiters();
            });

            let mut args = crate::Cli::parse_from(["localagent"]).run;
            args.provider = Some(ProviderKind::Mock);
            args.model = Some(model.clone());
            args.base_url = Some(base_url.clone());
            args.prompt = Some(prompt.clone());
            args.workdir = state_for_task.workdir.clone();
            args.state_dir = Some(state_for_task.paths.state_dir.clone());
            args.session = session_id_for_task.clone();
            args.no_session = false;
            args.stream = false;
            args.tui = false;
            args.disable_implementation_guard = true;

            let (ui_tx, ui_rx) = std::sync::mpsc::channel::<crate::events::Event>();
            let event_state = state_for_task.clone();
            let event_run_id = run_id_for_task.clone();
            let event_thread = std::thread::spawn(move || {
                let mut sequence = 0u64;
                while let Ok(event) = ui_rx.recv() {
                    sequence = sequence.saturating_add(1);
                    if let Some(projected) = crate::events::project_event_v1(&event, sequence) {
                        update_run_record(&event_state, &event_run_id, |record| {
                            record.projected_events.push(projected);
                            record.event_notify.notify_waiters();
                        });
                    }
                }
            });

            let result = crate::run_agent_with_ui(
                DelayedTestProvider { delay_ms },
                ProviderKind::Mock,
                &base_url,
                &model,
                &prompt,
                &args,
                &state_for_task.paths,
                Some(ui_tx),
                Some(input_rx),
                Some((cancel_tx, cancel_rx)),
                None,
                true,
            )
            .await;
            let _ = event_thread.join();

            match result {
                Ok(exec) => {
                    let terminal_status =
                        super::terminal_status_for_exit_reason(exec.outcome.exit_reason)
                            .to_string();
                    let run_artifact_path = exec
                        .run_artifact_path
                        .as_ref()
                        .map(|path| path.display().to_string());
                    update_run_record(&state_for_task, &run_id_for_task, |record| {
                        record.status = terminal_status.clone();
                        record.finished_at = Some(exec.outcome.finished_at.clone());
                        record.runtime_run_id = Some(exec.outcome.run_id.clone());
                        record.exit_reason = Some(exec.outcome.exit_reason.as_str().to_string());
                        record.error = exec.outcome.error.clone();
                        record.run_artifact_path = run_artifact_path.clone();
                        record.input_tx = None;
                        record.cancel_tx = None;
                        record.event_notify.notify_waiters();
                    });
                }
                Err(err) => {
                    update_run_record(&state_for_task, &run_id_for_task, |record| {
                        record.status = "failed".to_string();
                        record.finished_at = Some(crate::trust::now_rfc3339());
                        record.exit_reason =
                            Some(crate::AgentExitReason::ProviderError.as_str().to_string());
                        record.error = Some(err.to_string());
                        record.input_tx = None;
                        record.cancel_tx = None;
                        record.event_notify.notify_waiters();
                    });
                }
            }

            let mut sessions = state_for_task
                .sessions
                .lock()
                .expect("session registry lock");
            if let Some(record) = sessions
                .iter_mut()
                .find(|record| record.session_id == session_id_for_task)
            {
                record.active_run_id = None;
                record.last_run_id = Some(run_id_for_task.clone());
                record.updated_at = crate::trust::now_rfc3339();
            }
        });

        (session_id, run_id)
    }

    #[tokio::test]
    async fn server_info_route_returns_structured_payload() {
        let app = build_router(sample_state());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/server")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let payload: serde_json::Value = serde_json::from_slice(&body).expect("server info");
        assert_eq!(payload["schema_version"], "v1");
        assert_eq!(payload["backend_instance_id"], "b_test");
        assert_eq!(payload["status"], "ready");
        assert_eq!(payload["transport"], "http");
    }

    #[tokio::test]
    async fn server_capabilities_route_returns_structured_payload() {
        let app = build_router(sample_state());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/server/capabilities")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("server capabilities");
        assert_eq!(payload["schema_version"], "v1");
        assert_eq!(payload["attach"], true);
        assert_eq!(payload["session_registry"], true);
        assert_eq!(payload["event_stream"], true);
    }

    #[tokio::test]
    async fn create_list_and_get_session_routes_round_trip() {
        let app = build_router(sample_state());

        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"session_name":"default"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(created.status(), StatusCode::CREATED);
        let created_body = axum::body::to_bytes(created.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let created_payload: serde_json::Value =
            serde_json::from_slice(&created_body).expect("session created");
        let session_id = created_payload["session_id"]
            .as_str()
            .expect("session id")
            .to_string();

        let listed = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/sessions")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(listed.status(), StatusCode::OK);
        let listed_body = axum::body::to_bytes(listed.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let listed_payload: serde_json::Value =
            serde_json::from_slice(&listed_body).expect("session list");
        assert_eq!(
            listed_payload["sessions"]
                .as_array()
                .expect("sessions")
                .len(),
            1
        );
        assert_eq!(listed_payload["sessions"][0]["session_name"], "default");

        let fetched = app
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{session_id}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(fetched.status(), StatusCode::OK);
        let fetched_body = axum::body::to_bytes(fetched.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let fetched_payload: serde_json::Value =
            serde_json::from_slice(&fetched_body).expect("session info");
        assert_eq!(fetched_payload["session_id"], session_id);
        assert_eq!(fetched_payload["state_mode"], "persistent");
    }

    #[tokio::test]
    async fn get_missing_session_returns_structured_not_found() {
        let app = build_router(sample_state());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/sessions/s_missing")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let payload: serde_json::Value = serde_json::from_slice(&body).expect("error payload");
        assert_eq!(payload["schema_version"], "v1");
        assert_eq!(payload["error"]["code"], "SESSION_NOT_FOUND");
        assert_eq!(payload["error"]["backend_instance_id"], "b_test");
    }

    #[tokio::test]
    async fn create_run_marks_session_active_and_returns_run_identity() {
        let app = build_router(sample_state());

        let created_session = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"session_name":"default"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        let created_session_body = axum::body::to_bytes(created_session.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let created_session_payload: serde_json::Value =
            serde_json::from_slice(&created_session_body).expect("session created");
        let session_id = created_session_payload["session_id"]
            .as_str()
            .expect("session id")
            .to_string();

        let created_run = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/sessions/{session_id}/runs"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"prompt":"Say hi.","provider":"mock","model":"example-model"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(created_run.status(), StatusCode::CREATED);
        let created_run_body = axum::body::to_bytes(created_run.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let created_run_payload: serde_json::Value =
            serde_json::from_slice(&created_run_body).expect("run created");
        let run_id = created_run_payload["run_id"]
            .as_str()
            .expect("run id")
            .to_string();
        assert_eq!(created_run_payload["status"], "running");
        assert_eq!(created_run_payload["session_id"], session_id);

        let fetched_session_app = app.clone();

        let mut saw_created = false;
        let mut saw_running = false;
        let mut saw_finished = false;
        for _ in 0..50 {
            let fetched_run = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/v1/runs/{run_id}"))
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(fetched_run.status(), StatusCode::OK);
            let fetched_run_body = axum::body::to_bytes(fetched_run.into_body(), usize::MAX)
                .await
                .expect("bytes");
            let fetched_run_payload: serde_json::Value =
                serde_json::from_slice(&fetched_run_body).expect("run info");
            if fetched_run_payload["status"] == "created" {
                saw_created = true;
            }
            if fetched_run_payload["status"] == "running" {
                saw_running = true;
            }
            if fetched_run_payload["status"] == "finished" {
                saw_finished = true;
                assert_eq!(fetched_run_payload["exit_reason"], "ok");
                assert!(fetched_run_payload["runtime_run_id"].as_str().is_some());
                break;
            }
            if fetched_run_payload["status"] == "failed" {
                panic!("run failed unexpectedly: {fetched_run_payload}");
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(saw_created || saw_running || saw_finished);
        assert!(saw_finished);

        let fetched_session = fetched_session_app
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{session_id}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(fetched_session.status(), StatusCode::OK);
        let fetched_session_body = axum::body::to_bytes(fetched_session.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let fetched_session_payload: serde_json::Value =
            serde_json::from_slice(&fetched_session_body).expect("session info");
        assert_eq!(
            fetched_session_payload["active_run_id"],
            serde_json::Value::Null
        );
        assert_eq!(fetched_session_payload["last_run_id"], run_id);
    }

    #[tokio::test]
    async fn attach_session_reports_backend_owned_run_identity() {
        let app = build_router(sample_state());

        let created_session = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"session_name":"default"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        let created_session_body = axum::body::to_bytes(created_session.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let created_session_payload: serde_json::Value =
            serde_json::from_slice(&created_session_body).expect("session created");
        let session_id = created_session_payload["session_id"]
            .as_str()
            .expect("session id")
            .to_string();

        let created_run = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/sessions/{session_id}/runs"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"prompt":"Say hi.","provider":"mock","model":"example-model"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        let created_run_body = axum::body::to_bytes(created_run.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let created_run_payload: serde_json::Value =
            serde_json::from_slice(&created_run_body).expect("run created");
        let run_id = created_run_payload["run_id"]
            .as_str()
            .expect("run id")
            .to_string();

        for _ in 0..50 {
            let attached = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(Method::POST)
                        .uri(format!("/v1/sessions/{session_id}/attach"))
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(attached.status(), StatusCode::OK);
            let attached_body = axum::body::to_bytes(attached.into_body(), usize::MAX)
                .await
                .expect("bytes");
            let attached_payload: serde_json::Value =
                serde_json::from_slice(&attached_body).expect("attach payload");
            let active = &attached_payload["active_run_id"];
            let last = &attached_payload["last_run_id"];
            if active == &serde_json::Value::String(run_id.clone())
                || last == &serde_json::Value::String(run_id.clone())
            {
                assert!(
                    attached_payload["next_event_sequence"]
                        .as_u64()
                        .unwrap_or(0)
                        >= 1
                );
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        panic!("attach never exposed backend-owned run identity");
    }

    #[tokio::test]
    async fn run_events_route_replays_projected_runtime_events() {
        let app = build_router(sample_state());

        let created_session = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"session_name":"default"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        let created_session_body = axum::body::to_bytes(created_session.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let created_session_payload: serde_json::Value =
            serde_json::from_slice(&created_session_body).expect("session created");
        let session_id = created_session_payload["session_id"]
            .as_str()
            .expect("session id")
            .to_string();

        let created_run = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/sessions/{session_id}/runs"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"prompt":"Say hi.","provider":"mock","model":"example-model"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        let created_run_body = axum::body::to_bytes(created_run.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let created_run_payload: serde_json::Value =
            serde_json::from_slice(&created_run_body).expect("run created");
        let run_id = created_run_payload["run_id"]
            .as_str()
            .expect("run id")
            .to_string();

        for _ in 0..50 {
            let events_response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/v1/runs/{run_id}/events"))
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(events_response.status(), StatusCode::OK);
            let events_body = axum::body::to_bytes(events_response.into_body(), usize::MAX)
                .await
                .expect("bytes");
            let events_payload: serde_json::Value =
                serde_json::from_slice(&events_body).expect("events payload");
            let event_types = events_payload["events"]
                .as_array()
                .expect("events")
                .iter()
                .filter_map(|event| event["type"].as_str())
                .collect::<Vec<_>>();
            if event_types.contains(&"run_started") && event_types.contains(&"run_finished") {
                let after_one = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .uri(format!("/v1/runs/{run_id}/events?after=1"))
                            .body(Body::empty())
                            .expect("request"),
                    )
                    .await
                    .expect("response");
                assert_eq!(after_one.status(), StatusCode::OK);
                let after_one_body = axum::body::to_bytes(after_one.into_body(), usize::MAX)
                    .await
                    .expect("bytes");
                let after_one_payload: serde_json::Value =
                    serde_json::from_slice(&after_one_body).expect("events payload");
                assert!(
                    after_one_payload["events"]
                        .as_array()
                        .expect("events")
                        .len()
                        <= event_types.len()
                );
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        panic!("did not observe projected run_started/run_finished events");
    }

    #[tokio::test]
    async fn create_run_for_missing_session_returns_structured_not_found() {
        let app = build_router(sample_state());
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/sessions/s_missing/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"prompt":"Say hi.","provider":"mock","model":"example-model"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let payload: serde_json::Value = serde_json::from_slice(&body).expect("error payload");
        assert_eq!(payload["error"]["code"], "SESSION_NOT_FOUND");
    }

    #[tokio::test]
    async fn get_missing_run_returns_structured_not_found() {
        let app = build_router(sample_state());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/runs/r_missing")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let payload: serde_json::Value = serde_json::from_slice(&body).expect("error payload");
        assert_eq!(payload["error"]["code"], "RUN_NOT_FOUND");
    }

    #[tokio::test]
    async fn stream_run_events_route_uses_sse_content_type() {
        let state = sample_state();
        let (run_id, _input_rx, _cancel_rx) = insert_running_run(&state);
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/runs/{run_id}/events/stream"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(content_type.starts_with("text/event-stream"));
    }

    #[tokio::test]
    async fn submit_run_input_routes_into_backend_operator_queue() {
        let state = sample_state();
        let (run_id, input_rx, _cancel_rx) = insert_running_run(&state);
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/runs/{run_id}/input"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind":"steer","content":"change course after this tool"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let queued = input_rx.recv().expect("queued operator input");
        assert_eq!(queued.kind, crate::operator_queue::QueueMessageKind::Steer);
        assert_eq!(queued.content, "change course after this tool");
    }

    #[tokio::test]
    async fn cancel_run_signals_backend_cancel_channel() {
        let state = sample_state();
        let (run_id, _input_rx, mut cancel_rx) = insert_running_run(&state);
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/runs/{run_id}/cancel"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        cancel_rx.changed().await.expect("cancel signal");
        assert!(*cancel_rx.borrow());
    }

    #[tokio::test]
    async fn interrupt_route_queues_steer_message() {
        let state = sample_state();
        let (run_id, input_rx, _cancel_rx) = insert_running_run(&state);
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/runs/{run_id}/interrupt"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"content":"stop and change direction"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let queued = input_rx.recv().expect("queued interrupt");
        assert_eq!(queued.kind, crate::operator_queue::QueueMessageKind::Steer);
        assert_eq!(queued.content, "stop and change direction");
    }

    #[tokio::test]
    async fn next_route_queues_follow_up_message() {
        let state = sample_state();
        let (run_id, input_rx, _cancel_rx) = insert_running_run(&state);
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/runs/{run_id}/next"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"content":"after this turn, check tests"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let queued = input_rx.recv().expect("queued next");
        assert_eq!(
            queued.kind,
            crate::operator_queue::QueueMessageKind::FollowUp
        );
        assert_eq!(queued.content, "after this turn, check tests");
    }

    #[test]
    fn parse_sse_data_frame_ignores_keepalive_and_extracts_run_event_json() {
        assert_eq!(
            super::parse_sse_data_frame("event: keepalive\ndata: {}\n"),
            None
        );
        let payload =
            super::parse_sse_data_frame("id: 7\nevent: run_event\ndata: {\"sequence\":7}\n\n")
                .expect("run event payload");
        assert_eq!(payload, "{\"sequence\":7}");
    }

    #[tokio::test]
    async fn cancelled_run_persists_stable_artifact() {
        let state = sample_state();
        let app = build_router(state.clone());
        let (session_id, run_id) = spawn_delayed_server_run(&state, 250);

        for _ in 0..50 {
            let run = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/v1/runs/{run_id}"))
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            let body = axum::body::to_bytes(run.into_body(), usize::MAX)
                .await
                .expect("bytes");
            let payload: serde_json::Value = serde_json::from_slice(&body).expect("run payload");
            if payload["status"] == "running" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let cancel = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/runs/{run_id}/cancel"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(cancel.status(), StatusCode::ACCEPTED);

        let (artifact_path, runtime_run_id) = loop {
            let run = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/v1/runs/{run_id}"))
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            let body = axum::body::to_bytes(run.into_body(), usize::MAX)
                .await
                .expect("bytes");
            let payload: serde_json::Value = serde_json::from_slice(&body).expect("run payload");
            if payload["status"] == "cancelled" {
                assert_eq!(payload["exit_reason"], "cancelled");
                let artifact_path = payload["run_artifact_path"]
                    .as_str()
                    .expect("artifact path")
                    .to_string();
                let runtime_run_id = payload["runtime_run_id"]
                    .as_str()
                    .expect("runtime run id")
                    .to_string();
                break (artifact_path, runtime_run_id);
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        };

        let session = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{session_id}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let session_body = axum::body::to_bytes(session.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let session_payload: serde_json::Value =
            serde_json::from_slice(&session_body).expect("session payload");
        assert_eq!(session_payload["active_run_id"], serde_json::Value::Null);
        assert_eq!(session_payload["last_run_id"], run_id);

        let artifact_text =
            std::fs::read_to_string(&artifact_path).expect("read cancelled run artifact");
        let artifact_json: serde_json::Value =
            serde_json::from_str(&artifact_text).expect("cancelled run artifact json");
        assert_eq!(artifact_json["metadata"]["exit_reason"], "cancelled");
        assert_eq!(artifact_json["metadata"]["run_id"], runtime_run_id);
        assert!(artifact_json["metadata"]["finished_at"].as_str().is_some());
    }

    #[tokio::test]
    async fn late_cancel_does_not_corrupt_completed_artifact() {
        let state = sample_state();
        let app = build_router(state.clone());
        let (_session_id, run_id) = spawn_delayed_server_run(&state, 10);

        let (artifact_path, artifact_before, runtime_run_id) = loop {
            let run = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/v1/runs/{run_id}"))
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            let body = axum::body::to_bytes(run.into_body(), usize::MAX)
                .await
                .expect("bytes");
            let payload: serde_json::Value = serde_json::from_slice(&body).expect("run payload");
            if payload["status"] == "finished" {
                let artifact_path = payload["run_artifact_path"]
                    .as_str()
                    .expect("artifact path")
                    .to_string();
                let runtime_run_id = payload["runtime_run_id"]
                    .as_str()
                    .expect("runtime run id")
                    .to_string();
                let artifact_before =
                    std::fs::read_to_string(&artifact_path).expect("read finished artifact");
                break (artifact_path, artifact_before, runtime_run_id);
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        };

        let cancel = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/runs/{run_id}/cancel"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(cancel.status(), StatusCode::CONFLICT);
        let cancel_body = axum::body::to_bytes(cancel.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let cancel_payload: serde_json::Value =
            serde_json::from_slice(&cancel_body).expect("cancel payload");
        assert_eq!(cancel_payload["error"]["code"], "RUN_NOT_ACTIVE");

        let run = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/runs/{run_id}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let body = axum::body::to_bytes(run.into_body(), usize::MAX)
            .await
            .expect("bytes");
        let payload: serde_json::Value = serde_json::from_slice(&body).expect("run payload");
        assert_eq!(payload["status"], "finished");
        assert_eq!(payload["exit_reason"], "ok");
        assert_eq!(payload["runtime_run_id"], runtime_run_id);

        let artifact_after =
            std::fs::read_to_string(&artifact_path).expect("read finished artifact after cancel");
        let artifact_json: serde_json::Value =
            serde_json::from_str(&artifact_after).expect("artifact json");
        assert_eq!(artifact_json["metadata"]["run_id"], runtime_run_id);
        assert_eq!(artifact_before, artifact_after);
    }
}

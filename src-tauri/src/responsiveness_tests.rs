use super::*;
use std::sync::mpsc;
use tauri::test::{mock_builder, mock_context, noop_assets, INVOKE_KEY};

// Exercise the real command dispatcher: a stalled network reader must not hold
// the thread that dispatches window operations (minimize, resize, etc.).
#[test]
fn network_command_releases_dispatcher_while_sampler_is_busy() {
    let app = mock_builder()
        .manage(AppState {
            networks: Mutex::new(Networks::new()).into(),
            ws_task: TokioMutex::new(None),
            tray_items: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![get_network_stats])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let handle = app.handle().clone();
    let (locked_tx, locked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let state = handle.state::<AppState>();
        let _guard = state.networks.lock().unwrap();
        locked_tx.send(()).unwrap();
        // The watchdog avoids hanging the regression test on the old code.
        release_rx.recv_timeout(Duration::from_secs(2)).is_ok()
    });
    locked_rx.recv().unwrap();
    let (response_tx, response_rx) = mpsc::channel();
    window.as_ref().clone().on_message(
        tauri::webview::InvokeRequest {
            cmd: "get_network_stats".into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::default(),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.into(),
        },
        Box::new(move |_, _, response, _, _| {
            response_tx.send(response).unwrap();
        }),
    );
    let _ = release_tx.send(());
    assert!(worker.join().unwrap(), "network command blocked the UI dispatcher until the watchdog released the sampler");
    let response = response_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    match response {
        tauri::ipc::InvokeResponse::Ok(body) => {
            body.deserialize::<(u64, u64)>().expect("retain the frontend's counter-pair contract");
        }
        tauri::ipc::InvokeResponse::Err(error) => panic!("network command failed: {error:?}"),
    }
}

//! Browser: a hidden tab gets no more animation frames, and Bevy's loop only updates on those, so
//! the game stops — the partner, hearing nothing, drops us after `DISCONNECT_TIMEOUT` (two tabs of
//! the same window: one of them is always hidden). While the co-op is on, a worker's clock wakes
//! the event loop in a hidden tab (its messages aren't throttled like the tab's own timers).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::winit::{EventLoopProxyWrapper, WinitUserEvent};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{Blob, BlobPropertyBag, Url, Worker, js_sys};

use super::Net;

/// Ticks while the tab is hidden (posted `true`), stops otherwise.
const WORKER: &str = "let t; onmessage = (e) => { clearInterval(t); if (e.data) t = setInterval(() => postMessage(0), 16); };";
/// While hidden, the updates may come late: the simulation catches up (rather than falling behind).
const HIDDEN_MAX_DELTA: Duration = Duration::from_secs(1);
const MAX_DELTA: Duration = Duration::from_millis(250);

/// A search or a co-op game is going on (the network must keep up).
static ACTIVE: AtomicBool = AtomicBool::new(false);

fn hidden() -> bool {
    web_sys::window().and_then(|w| w.document()).is_some_and(|d| d.hidden())
}

/// Starts the worker, and makes it tick while the tab is hidden.
pub fn install(proxy: Res<EventLoopProxyWrapper>) {
    if let Err(e) = start((**proxy).clone()) {
        warn!("co-op: no background clock ({e:?}), a hidden tab will be dropped");
    }
}

fn start(proxy: bevy::winit::EventLoopProxy<WinitUserEvent>) -> Result<(), JsValue> {
    let opts = BlobPropertyBag::new();
    opts.set_type("text/javascript");
    let blob = Blob::new_with_str_sequence_and_options(&js_sys::Array::of1(&WORKER.into()), &opts)?;
    let worker = Worker::new(&Url::create_object_url_with_blob(&blob)?)?;
    let on_tick = Closure::<dyn FnMut()>::new(move || {
        if ACTIVE.load(Ordering::Relaxed) {
            let _ = proxy.send_event(WinitUserEvent::WakeUp);
        }
    });
    worker.set_onmessage(Some(on_tick.as_ref().unchecked_ref()));
    on_tick.forget();
    let document = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
    let ticker = worker.clone();
    let on_visibility = Closure::<dyn FnMut()>::new(move || {
        let _ = ticker.post_message(&hidden().into());
    });
    document.add_event_listener_with_callback("visibilitychange", on_visibility.as_ref().unchecked_ref())?;
    on_visibility.forget();
    worker.post_message(&hidden().into())?;
    std::mem::forget(worker);
    Ok(())
}

/// Bevy only updates without an animation frame if its window is "invisible" (a no-op in the
/// browser): it must already be when the tab gets hidden, the loop being stopped by then.
pub fn follow(net: Res<Net>, mut windows: Query<&mut Window, With<PrimaryWindow>>, mut time: ResMut<Time<Virtual>>) {
    let active = net.socket.is_some();
    ACTIVE.store(active, Ordering::Relaxed);
    for mut w in &mut windows {
        if w.visible == active {
            w.visible = !active;
        }
    }
    let max = if active && hidden() { HIDDEN_MAX_DELTA } else { MAX_DELTA };
    if time.max_delta() != max {
        time.set_max_delta(max);
    }
}

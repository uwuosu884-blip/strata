//! Touchpad pinch → egui zoom on Wayland (winit 0.30 only reports pinch
//! gestures on macOS; Windows sends Ctrl+wheel). A second event queue on the
//! app's own `wl_display` binds `zwp_pointer_gestures_v1` and accumulates the
//! pinch scale; `take()` hands the factor to `raw_input_hook` as
//! `egui::Event::Zoom`, so every view zooms exactly like Ctrl+wheel.

use std::sync::Mutex;

/// Pinch scale gathered since the last frame (1.0 = none).
static SCALE: Mutex<f32> = Mutex::new(1.0);

/// Zoom factor gathered since the last call (None = no pinch).
pub fn take() -> Option<f32> {
    let mut g = SCALE.lock().unwrap_or_else(|e| e.into_inner());
    let f = std::mem::replace(&mut *g, 1.0);
    (f.is_finite() && f > 0.0 && (f - 1.0).abs() > 1e-4).then_some(f)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn add(factor: f32) {
    let mut g = SCALE.lock().unwrap_or_else(|e| e.into_inner());
    *g = (*g * factor).clamp(0.01, 100.0);
}

#[cfg(target_os = "linux")]
mod imp {
    use eframe::egui;
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::{wl_pointer, wl_registry, wl_seat};
    use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};
    use wayland_protocols::wp::pointer_gestures::zv1::client::{
        zwp_pointer_gesture_pinch_v1 as pinch_v1, zwp_pointer_gestures_v1 as gestures_v1,
    };

    struct State {
        ctx: egui::Context,
        gestures: Option<gestures_v1::ZwpPointerGesturesV1>,
        pointer: Option<wl_pointer::WlPointer>,
        pinch: Option<pinch_v1::ZwpPointerGesturePinchV1>,
        /// Scale reported by the previous update of this gesture.
        last: f64,
    }

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<wl_seat::WlSeat, ()> for State {
        fn event(st: &mut Self, seat: &wl_seat::WlSeat, ev: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
            if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(c) } = ev
                && c.contains(wl_seat::Capability::Pointer)
                && st.pointer.is_none()
                && let Some(g) = &st.gestures
            {
                let p = seat.get_pointer(qh, ());
                st.pinch = Some(g.get_pinch_gesture(&p, qh, ()));
                st.pointer = Some(p);
            }
        }
    }

    impl Dispatch<wl_pointer::WlPointer, ()> for State {
        fn event(_: &mut Self, _: &wl_pointer::WlPointer, _: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
    }

    impl Dispatch<gestures_v1::ZwpPointerGesturesV1, ()> for State {
        fn event(
            _: &mut Self,
            _: &gestures_v1::ZwpPointerGesturesV1,
            _: gestures_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<pinch_v1::ZwpPointerGesturePinchV1, ()> for State {
        fn event(
            st: &mut Self,
            _: &pinch_v1::ZwpPointerGesturePinchV1,
            ev: pinch_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match ev {
                pinch_v1::Event::Begin { .. } => st.last = 1.0,
                pinch_v1::Event::Update { scale, .. } => {
                    // `scale` is relative to the gesture start.
                    if scale > 0.0 && st.last > 0.0 {
                        super::add((scale / st.last) as f32);
                        st.ctx.request_repaint();
                    }
                    st.last = scale;
                }
                pinch_v1::Event::End { .. } => st.last = 1.0,
                _ => {}
            }
        }
    }

    /// Start listening on the app's Wayland display (no-op on X11 / failure).
    pub fn start(cc: &eframe::CreationContext<'_>) {
        use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
        let Ok(h) = cc.display_handle() else { return };
        let RawDisplayHandle::Wayland(w) = h.as_raw() else { return };
        let ptr = w.display.as_ptr();
        // SAFETY: the display outlives the app (winit owns it for the whole
        // run); the foreign backend only adds our own queue and objects.
        let backend = unsafe { wayland_backend::client::Backend::from_foreign_display(ptr.cast()) };
        let conn = Connection::from_backend(backend);
        let ctx = cc.egui_ctx.clone();
        let spawned = std::thread::Builder::new().name("daw-pinch".into()).spawn(move || {
            let Ok((globals, mut queue)) = registry_queue_init::<State>(&conn) else { return };
            let qh = queue.handle();
            let gestures = globals.bind::<gestures_v1::ZwpPointerGesturesV1, _, _>(&qh, 1..=3, ()).ok();
            if gestures.is_none() {
                return; // compositor without pointer gestures
            }
            let mut st = State { ctx, gestures, pointer: None, pinch: None, last: 1.0 };
            if globals.bind::<wl_seat::WlSeat, _, _>(&qh, 1..=5, ()).is_err() {
                return;
            }
            while queue.blocking_dispatch(&mut st).is_ok() {}
        });
        if let Err(e) = spawned {
            eprintln!("pinch gestures unavailable: {e}");
        }
    }
}

#[cfg(target_os = "linux")]
pub use imp::start;

#[cfg(not(target_os = "linux"))]
pub fn start(_: &eframe::CreationContext<'_>) {}

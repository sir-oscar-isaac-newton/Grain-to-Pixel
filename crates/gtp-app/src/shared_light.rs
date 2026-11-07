//! The one light of the app, shared by the commands, the capture jobs and the temperature
//! watchdog (D-04, docs/specification.md section 6.1).
//!
//! [`SharedLight`] implements [`LightSource`] but **locks the light per call**, never for a whole
//! job: a capture sequence drives it through a handle like any other light, and between two of its
//! calls (during the settle wait, or the long `wait_for_image` of the camera) the lock is free, so
//! the watchdog and `light_off` always get through. The lock is never held across a sleep or a
//! camera call; the one place it is held for long is `connect`, which switches the light off first.
//!
//! The watchdog is [`SharedLight::poll`]: it reads the light's temperature and, above the abort
//! limit, sets the **tripped** flag and switches the light off. While tripped, every non-zero
//! `set_levels` is refused (switching off still works), so the next colour of a running sequence
//! cannot light. The flag is cleared by `connect` only: the user reconnects the light, once it has
//! cooled down.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};

use gtp_capture::{LightInfo, LightSource, LightStatus, PortInfo};
use gtp_core::{AppError, ErrorCode};

/// A light that can be moved between threads.
pub type BoxedLight = Box<dyn LightSource + Send>;

/// Lists the serial ports the light may be on (`gtp_capture::list_ports`, or one fake port).
pub type PortLister = fn() -> Result<Vec<PortInfo>, AppError>;

struct Inner {
    light: Mutex<BoxedLight>,
    tripped: AtomicBool,
    /// The last `set_levels` that succeeded was non-zero (the light may be on).
    lit: AtomicBool,
    ports: PortLister,
}

/// A cheap handle on the app's light; clones share the light and the tripped flag.
#[derive(Clone)]
pub struct SharedLight(Arc<Inner>);

/// What the watchdog read from the light on one tick, for the `light://status` event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightReading {
    /// LED temperature in degrees Celsius, none before the light's first reading.
    pub temp_c: Option<f32>,
    /// Supply voltage in millivolts.
    pub vbus_mv: Option<i32>,
    /// The light was switched off for being too hot and stays refused until it is reconnected.
    pub tripped: bool,
}

/// The error of a light refused after a trip.
pub fn tripped_error() -> AppError {
    AppError::new(
        ErrorCode::Io,
        "The light was switched off because it got too hot or stopped reporting its temperature. Let it cool down, check its USB cable, then reconnect it.",
    )
}

impl SharedLight {
    /// Wraps `light`; `ports` answers `light_list`.
    pub fn new(light: BoxedLight, ports: PortLister) -> Self {
        Self(Arc::new(Inner {
            light: Mutex::new(light),
            tripped: AtomicBool::new(false),
            lit: AtomicBool::new(false),
            ports,
        }))
    }

    fn lock(&self) -> MutexGuard<'_, BoxedLight> {
        self.0.light.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The serial ports the light may be on.
    pub fn list_ports(&self) -> Result<Vec<PortInfo>, AppError> {
        (self.0.ports)()
    }

    /// Switches the light off. Always allowed, tripped or not, and always attempted: the driver
    /// marks the link down after one failed write while the light may still be on. A failure is
    /// reported unless the light says it is not connected (then there is nothing to switch off).
    pub fn off(&self) -> Result<(), AppError> {
        let mut light = self.lock();
        match light.set_levels([0; 3]) {
            Ok(()) => {
                self.0.lit.store(false, Ordering::SeqCst);
                Ok(())
            }
            Err(_) if !light.status().connected => {
                self.0.lit.store(false, Ordering::SeqCst);
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    /// For app exit: switches the light off and refuses every colour from now on (the flag is
    /// the trip's), so a capture still running while the app quits cannot light the next one.
    pub fn shut_down(&self) {
        self.0.tripped.store(true, Ordering::SeqCst);
        if let Err(err) = self.off() {
            tracing::error!("switching the light off at exit failed: {err}");
        }
    }

    /// True after the watchdog switched the light off for the temperature, until `connect`.
    pub fn is_tripped(&self) -> bool {
        self.0.tripped.load(Ordering::SeqCst)
    }

    /// One watchdog tick: reads the light and trips (flag first, then the off command, sent again
    /// on every tick while the condition lasts) when it reports a temperature above `abort_c`, or
    /// when it is lit and reports no temperature at all (a light that cannot be monitored must not
    /// stay on). `None` while the light is not connected; an unlit light with no reading never
    /// trips.
    pub fn poll(&self, abort_c: f32) -> Option<LightReading> {
        let status = self.lock().status();
        if !status.connected {
            return None;
        }
        let reading = status.temp_c.filter(|t| t.is_finite());
        let hot = reading.filter(|t| *t > abort_c);
        let blind = reading.is_none() && self.0.lit.load(Ordering::SeqCst);
        if hot.is_some() || blind {
            if !self.0.tripped.swap(true, Ordering::SeqCst) {
                tracing::error!(
                    ?reading,
                    abort_c,
                    "the light is too hot or silent: switching it off"
                );
            }
            if let Err(err) = self.off() {
                tracing::error!("switching the hot light off failed: {err}");
            }
        }
        Some(LightReading {
            temp_c: status.temp_c,
            vbus_mv: status.vbus_mv,
            tripped: self.is_tripped(),
        })
    }

    /// A best-effort switch-off for a panic hook: it never waits for the lock (a thread that holds
    /// it may be the one that panicked, or may be stuck), recovers a poisoned lock, and catches a
    /// panic of the light itself, since a panic inside a panic hook aborts the process. True when
    /// the off command was sent.
    pub fn emergency_off(&self) -> bool {
        let mut light = match self.0.light.try_lock() {
            Ok(light) => light,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        catch_unwind(AssertUnwindSafe(|| light.set_levels([0; 3]).is_ok())).unwrap_or(false)
    }
}

impl LightSource for SharedLight {
    /// Switches the light off, connects, and clears the tripped flag. The lock is held while the
    /// ports are probed (a blocking call), which is why the light is off first.
    fn connect(&mut self, port: Option<&str>) -> Result<LightInfo, AppError> {
        let mut light = self.lock();
        if let Err(err) = light.set_levels([0; 3]) {
            tracing::debug!("switching the light off before connecting failed: {err}");
        }
        let info = light.connect(port)?;
        self.0.tripped.store(false, Ordering::SeqCst);
        Ok(info)
    }

    /// The flag is checked with the lock held, so a colour cannot slip in between a trip and its
    /// off command.
    fn set_levels(&mut self, rgb: [u8; 3]) -> Result<(), AppError> {
        let mut light = self.lock();
        if rgb != [0; 3] && self.is_tripped() {
            return Err(tripped_error());
        }
        light.set_levels(rgb)?;
        self.0.lit.store(rgb != [0; 3], Ordering::SeqCst);
        Ok(())
    }

    fn shutter_pulse(&mut self, tens_of_ms: u8) -> Result<(), AppError> {
        self.lock().shutter_pulse(tens_of_ms)
    }

    fn status(&self) -> LightStatus {
        self.lock().status()
    }
}

/// One fake port, for `GTP_FAKE_HW=1` and tests.
pub fn fake_ports() -> Result<Vec<PortInfo>, AppError> {
    Ok(vec![PortInfo {
        name: "fake".to_owned(),
        likely: true,
    }])
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use gtp_capture::{FakeLight, FakeLightHandle, LightCommand};

    use super::*;

    fn rig() -> (SharedLight, FakeLightHandle) {
        let fake = FakeLight::new();
        let handle = fake.handle();
        (SharedLight::new(Box::new(fake), fake_ports), handle)
    }

    fn status(temp_c: Option<f32>) -> LightStatus {
        LightStatus {
            temp_c,
            vbus_mv: Some(5150),
            connected: true,
        }
    }

    #[test]
    fn commands_reach_the_light_and_clones_share_it() {
        let (mut light, handle) = rig();
        let mut other = light.clone();
        light.set_levels([44, 0, 0]).unwrap();
        assert_eq!(other.status().temp_c, Some(25.0));
        other.set_levels([0, 22, 0]).unwrap();
        assert_eq!(handle.levels(), [0, 22, 0]);
        light.off().unwrap();
        assert_eq!(handle.levels(), [0; 3]);
        assert_eq!(light.list_ports().unwrap()[0].name, "fake");
    }

    #[test]
    fn the_watchdog_trips_above_the_abort_limit_and_switches_a_lit_light_off() {
        let (mut light, handle) = rig();
        light.set_levels([44, 0, 0]).unwrap();
        handle.set_status(status(Some(60.0)));
        let reading = light.poll(55.0).unwrap();
        assert!(reading.tripped);
        assert_eq!(reading.temp_c, Some(60.0));
        assert_eq!(handle.levels(), [0; 3]);
        assert!(light.is_tripped());
    }

    #[test]
    fn the_abort_limit_itself_does_not_trip() {
        let (mut light, handle) = rig();
        light.set_levels([44, 0, 0]).unwrap();
        handle.set_status(status(Some(55.0)));
        assert!(!light.poll(55.0).unwrap().tripped);
        assert_eq!(handle.levels(), [44, 0, 0]);
    }

    #[test]
    fn an_unlit_light_with_no_reading_never_trips() {
        let (mut light, handle) = rig();
        handle.set_status(status(None));
        let reading = light.poll(55.0).unwrap();
        assert_eq!((reading.temp_c, reading.tripped), (None, false));
        handle.set_status(status(Some(f32::NAN)));
        assert!(!light.poll(55.0).unwrap().tripped);
        // Lit and then off again: still nothing to guard.
        light.set_levels([44, 0, 0]).unwrap();
        light.set_levels([0; 3]).unwrap();
        handle.set_status(status(None));
        assert!(!light.poll(55.0).unwrap().tripped);
        light.set_levels([44, 0, 0]).unwrap();
        light.off().unwrap();
        assert!(!light.poll(55.0).unwrap().tripped);
    }

    #[test]
    fn a_lit_light_that_reports_no_temperature_is_switched_off_and_tripped() {
        for silent in [None, Some(f32::NAN)] {
            let (mut light, handle) = rig();
            light.set_levels([0, 22, 0]).unwrap();
            handle.set_status(status(silent));
            let reading = light.poll(55.0).unwrap();
            assert!(reading.tripped);
            assert_eq!(handle.levels(), [0; 3]);
            let err = light.set_levels([0, 22, 0]).unwrap_err();
            assert!(err.message.contains("stopped reporting"), "{err}");
        }
    }

    #[test]
    fn switching_off_is_attempted_even_when_the_light_says_it_is_not_connected() {
        let (light, handle) = rig();
        handle.set_status(LightStatus::default());
        let before = handle.commands().len();
        light.off().unwrap();
        assert_eq!(
            handle.commands().len(),
            before + 1,
            "the write was attempted"
        );
        assert_eq!(
            handle.commands().last(),
            Some(&LightCommand::SetLevels([0; 3]))
        );
    }

    #[test]
    fn a_failed_off_is_an_error_while_connected_and_fine_when_not_connected() {
        let (light, handle) = rig();
        handle.fail_commands_with(Some(gtp_capture::fakes::unplugged()));
        assert!(light.off().is_err());
        handle.set_status(LightStatus::default());
        assert!(light.off().is_ok());
    }

    #[test]
    fn shutting_down_switches_the_light_off_and_nothing_lights_it_again() {
        let (mut light, handle) = rig();
        light.set_levels([0, 22, 0]).unwrap();
        light.shut_down();
        assert_eq!(handle.levels(), [0; 3]);
        assert!(light.set_levels([44, 0, 0]).is_err());
        assert_eq!(handle.levels(), [0; 3]);
    }

    #[test]
    fn a_light_that_is_not_connected_is_not_polled() {
        let (light, handle) = rig();
        handle.set_status(LightStatus::default());
        assert_eq!(light.poll(55.0), None);
    }

    #[test]
    fn after_a_trip_no_colour_lights_but_off_still_works() {
        let (mut light, handle) = rig();
        handle.set_status(status(Some(60.0)));
        light.poll(55.0);
        let err = light.set_levels([0, 22, 0]).unwrap_err();
        assert!(err.message.contains("too hot"), "{err}");
        assert_eq!(handle.levels(), [0; 3]);
        assert!(light.set_levels([0; 3]).is_ok());
        assert!(light.off().is_ok());
    }

    #[test]
    fn a_trip_that_is_still_hot_sends_the_off_command_again_on_the_next_tick() {
        let (mut light, handle) = rig();
        handle.set_status(status(Some(60.0)));
        light.poll(55.0);
        handle.fail_commands_with(Some(gtp_capture::fakes::unplugged()));
        light.poll(55.0);
        handle.fail_commands_with(None);
        light.set_levels([0; 3]).unwrap();
        let offs = |h: &FakeLightHandle| {
            h.commands()
                .iter()
                .filter(|c| **c == LightCommand::SetLevels([0; 3]))
                .count()
        };
        let before = offs(&handle);
        light.poll(55.0);
        assert_eq!(offs(&handle), before + 1);
    }

    #[test]
    fn reconnecting_clears_the_trip_and_nothing_else_does() {
        let (mut light, handle) = rig();
        handle.set_status(status(Some(60.0)));
        light.poll(55.0);
        handle.set_status(status(Some(30.0)));
        // Cooling down, polling and switching off do not clear it.
        light.poll(55.0);
        light.off().unwrap();
        assert!(light.is_tripped());
        assert!(light.set_levels([0, 22, 0]).is_err());

        light.connect(None).unwrap();
        assert!(!light.is_tripped());
        light.set_levels([0, 22, 0]).unwrap();
        assert_eq!(handle.levels(), [0, 22, 0]);
    }

    #[test]
    fn a_failed_reconnect_keeps_the_trip() {
        struct NoLight;
        impl LightSource for NoLight {
            fn connect(&mut self, _: Option<&str>) -> Result<LightInfo, AppError> {
                Err(AppError::new(ErrorCode::NotFound, "none"))
            }
            fn set_levels(&mut self, _: [u8; 3]) -> Result<(), AppError> {
                Ok(())
            }
            fn shutter_pulse(&mut self, _: u8) -> Result<(), AppError> {
                Ok(())
            }
            fn status(&self) -> LightStatus {
                status(Some(99.0))
            }
        }
        let mut light = SharedLight::new(Box::new(NoLight), fake_ports);
        light.poll(55.0);
        assert!(light.connect(None).is_err());
        assert!(light.is_tripped());
    }

    #[test]
    fn connecting_switches_the_light_off_first() {
        let (mut light, handle) = rig();
        light.set_levels([44, 0, 0]).unwrap();
        light.connect(Some("COM5")).unwrap();
        let commands = handle.commands();
        let connect = commands
            .iter()
            .position(|c| matches!(c, LightCommand::Connect(_)))
            .unwrap();
        assert_eq!(commands[connect - 1], LightCommand::SetLevels([0; 3]));
    }

    /// A light whose `set_levels` waits for a release, to hold the lock from a second thread.
    struct SlowLight {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    impl LightSource for SlowLight {
        fn connect(&mut self, _: Option<&str>) -> Result<LightInfo, AppError> {
            Err(AppError::internal("unused"))
        }
        fn set_levels(&mut self, _: [u8; 3]) -> Result<(), AppError> {
            let _ = self.entered.send(());
            let _ = self.release.recv_timeout(Duration::from_secs(30));
            Ok(())
        }
        fn shutter_pulse(&mut self, _: u8) -> Result<(), AppError> {
            Ok(())
        }
        fn status(&self) -> LightStatus {
            status(Some(25.0))
        }
    }

    #[test]
    fn the_emergency_switch_off_never_waits_for_a_held_lock() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let light = SharedLight::new(
            Box::new(SlowLight {
                entered: entered_tx,
                release: release_rx,
            }),
            fake_ports,
        );
        let holder = {
            let mut light = light.clone();
            std::thread::spawn(move || light.set_levels([0; 3]))
        };
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        // The lock is held right now; the hook gives up at once instead of deadlocking.
        assert!(!light.emergency_off());
        release_tx.send(()).unwrap();
        holder.join().unwrap().unwrap();
        // Free again: with the sender dropped the slow light returns at once.
        drop(release_tx);
        assert!(light.emergency_off());
    }

    #[test]
    fn the_emergency_switch_off_works_on_a_poisoned_lock_and_survives_a_panicking_light() {
        let (light, handle) = rig();
        let poison = light.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poison.0.light.lock();
            panic!("a thread died holding the light");
        })
        .join();
        assert!(light.0.light.is_poisoned());
        let mut with_colour = light.clone();
        assert!(with_colour.set_levels([44, 0, 0]).is_ok());
        assert!(light.emergency_off());
        assert_eq!(handle.levels(), [0; 3]);

        struct Panicking;
        impl LightSource for Panicking {
            fn connect(&mut self, _: Option<&str>) -> Result<LightInfo, AppError> {
                Err(AppError::internal("unused"))
            }
            fn set_levels(&mut self, _: [u8; 3]) -> Result<(), AppError> {
                panic!("the driver crashed")
            }
            fn shutter_pulse(&mut self, _: u8) -> Result<(), AppError> {
                Ok(())
            }
            fn status(&self) -> LightStatus {
                LightStatus::default()
            }
        }
        let crashing = SharedLight::new(Box::new(Panicking), fake_ports);
        assert!(!crashing.emergency_off());
    }
}

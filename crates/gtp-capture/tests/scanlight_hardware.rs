//! Needs a real Scanlight V4 on USB, so it is ignored. Run it on the Mac with
//! `cargo test -p gtp-capture --test scanlight_hardware -- --ignored`. Optional:
//! `GTP_SCANLIGHT_PORT=/dev/cu.usbmodem1101`. The red LED lights at a low level for half a
//! second. Proven on macOS only; on the author's Windows PC it fails with "did not answer".

use std::time::{Duration, Instant};

use gtp_capture::{LightSource, ScanlightV4};

#[test]
#[ignore = "needs a real Scanlight V4"]
fn a_real_light_answers_reports_readings_and_switches_off() {
    let port = std::env::var("GTP_SCANLIGHT_PORT").ok();
    let mut light = ScanlightV4::new();
    let info = light.connect(port.as_deref()).unwrap();
    assert!(info.recognised, "unexpected hardware ID: {info:?}");

    let until = Instant::now() + Duration::from_secs(3);
    while light.status().temp_c.is_none() || light.status().vbus_mv.is_none() {
        assert!(Instant::now() < until, "no LED_TEMP and VBUS within 3 s");
        std::thread::sleep(Duration::from_millis(50));
    }
    let status = light.status();
    assert!(status.connected);
    assert!(status.vbus_mv.is_some_and(|mv| (4000..6000).contains(&mv)));

    light.set_levels([16, 0, 0]).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    light.set_levels([0, 0, 0]).unwrap();
}

# Grain to Pixel: user guide

This guide is for people who use Grain to Pixel but do not build it. It covers installing, keeping your library safe, setting up the Nikon Coolscan LS-50, setting up the camera and Scanlight, and what does not work yet. Where a step has not been tried on real hardware, the guide says "not yet tried".

Grain to Pixel works on your computer only. It has no accounts and sends nothing about you or your pictures anywhere.

Contents:

1. [Install](#1-install)
2. [First start and your library](#2-first-start-and-your-library)
3. [Back up your library](#3-back-up-your-library)
4. [The Nikon Coolscan LS-50](#4-the-nikon-coolscan-ls-50)
5. [Camera capture with the Scanlight and the Nikon D600](#5-camera-capture-with-the-scanlight-and-the-nikon-d600)
6. [Known limitations](#6-known-limitations)
7. [Troubleshooting](#7-troubleshooting)

## 1. Install

The installers are not signed with a paid certificate, so Windows and macOS both show a warning the first time. The warning is about the missing signature, not about something found in the file. The page [install.md](install.md) shows every click. In short:

- **Windows 10 or 11 (64-bit).** Run `Grain to Pixel_<version>_x64-setup.exe`. If "Windows protected your PC" appears (SmartScreen), choose **More info**, then **Run anyway**. It installs for your user only and needs no administrator rights. Windows on ARM computers may run it through Windows' emulation, but that is untested and unsupported, and there is no native ARM version.
- **macOS 13 or later on Apple silicon.** Open `Grain to Pixel_<version>_aarch64.dmg` and drag the app into **Applications**. Intel Macs are not supported. The first time, macOS says it cannot verify the app (Gatekeeper). Click **Done**, then open **System Settings > Privacy & Security** and click **Open Anyway**. You need this once.
- **Updates.** The app asks GitHub once a day at most whether a newer version exists. It installs nothing until you press **Install**. You can switch this off with **Check for updates** in the feature flags in **Settings**. Details are in [install.md](install.md).

Removing the app never removes your library or your originals.

## 2. First start and your library

The first time, the app shows **Set up your library**. Your library is one folder that holds your scans, thumbnails and catalog.

1. Choose **Create a new library** (or **Open an existing library** if you already have one).
2. Press **Choose folder…** and pick an empty folder on a disk with room. The screen shows the free space. A frame needs about 60 to 100 MB, so a 36-frame roll takes 2 to 4 GB.
3. Type a **Library name** and press **Create library**.

What is in the folder:

| Item | What it is |
|---|---|
| `library.db` | The catalog: rolls, frames, tags and every edit you made. |
| `originals/` | Your scans and exposures, kept exactly as they came in. |
| `thumbs/` | Small preview pictures. The app can make them again. |
| `exports/` | The default place for exported pictures. |

Your originals are never changed. Editing only changes the saved recipe in the catalog, and the files in `originals/` are marked read-only. Every original is checked against its fingerprint after it is copied in. To check them again later, open **Settings** and press **Verify library**. It only reports; it never changes or deletes anything.

The app remembers the last library you opened. A library made by a newer version of the app opens read-only: you can look at it, not change it.

## 3. Back up your library

A backup is a copy of the library folder.

1. Quit Grain to Pixel. (The catalog is a database with extra files beside it. Copying while the app runs can catch it half-written.)
2. Copy the **whole library folder** to another disk. Copy the folder itself, not single files from it.
3. Do this after every session you would hate to lose.

To restore or to move the library to another disk:

1. Quit the app and copy the folder to its new place.
2. Start the app. If it cannot find the library it remembers, it shows **Set up your library**.
3. Choose **Open an existing library**, press **Choose folder…**, pick the copied folder and press **Open library**.

When a new version of the app needs to change the catalog's layout, it first saves a copy of the catalog next to it as `library.db.bak-v<number>`. If you go back to an older app version, you can restore from that copy. Keep your own backups as well. There is no cloud sync.

## 4. The Nikon Coolscan LS-50

The app scans through a small program called `nkscan`, version 0.17.0. The app does not ship it and does not install it for you. It looks for a program named `nkscan` on your computer's search path, or at the place named by the `GTP_NKSCAN` setting. Any other version is refused.

The author's installation uses Rust to build it:

```bash
cargo install nkscan --version 0.17.0 --features cli --locked --root <a folder of your choice>
```

Then put that folder's `bin` folder on your search path. A ready-made download of `nkscan` for non-developers is not yet part of this project.

The scanner the app was tested with is the author's LS-50: it has the coolscan-mods firmware and an SA-21 jumpered as an SA-30 roll adapter, so it scans a whole roll of up to 40 frames. Other scanners and other LS-50 modifications are not supported. Flashing the firmware is a one-time job outside the app.

### Windows: the WinUSB driver

On Windows `nkscan` can only talk to the scanner when the scanner uses the **WinUSB** driver. With VueScan's driver (or another program's), the scanner is listed but cannot be used, and the app says "incompatible driver".

To switch to WinUSB (these are the steps the author used; they are from [research/nkscan.md](research/nkscan.md)):

1. Close VueScan and any other scanning program.
2. Use the program Zadig to install the WinUSB driver for the "LS-50" device.
3. In the app, open **Import** and look for **Nikon LS-50 ED**.

To go back to VueScan's driver (the firmware flasher needs it): open Windows **Device Manager**, find the scanner, and use **Roll Back Driver**. The WinUSB package stays in the driver store until you remove it with `pnputil /delete-driver oem62.inf`. The number in that name came from the author's computer and will differ on yours. Look it up before you delete anything.

If you prefer to keep VueScan's driver, you can scan with VueScan or Nikon Scan and import the TIFF files instead (**Files**, then **TIFF scans**).

### macOS

The author found the scanner with no driver set-up on a MacBook with an M1 Pro chip and scanned one frame on it (2000 dpi, with infrared). Scanning several frames in a row on macOS is not yet tried.

### Scanning a roll

1. Switch the scanner on, connect it by USB and load the roll.
2. Open **Import**. Choose a roll (or **Create roll** for a new one). Under "Where do the frames come from?" choose **Scanner**.
3. On the **Scanner** page the app shows "Scanner: …". If not, press **Look again**.
4. On the **Frames** page you can press **Preview the roll** to see the whole roll. Then set **First frame** and **Last frame**. The app continues after the last frame already scanned into the roll.
5. On the **Settings** page choose the **Resolution**, the **Samples per pixel** and whether to tick **Also scan the infrared (for dust removal)**.
6. On the **Scan** page press **Scan**. Each frame is stored as soon as it is scanned. If you cancel or an error happens, the frames before it stay in the roll, and you can continue from the next one.
7. Press **Finish** when the roll is done.

What to expect, from the author's tests: loading the film takes about 40 seconds. A single frame at 4000 dpi with infrared took 4.6 minutes from start to finish. At 2000 dpi each further frame took about 76 seconds. The scanner refocuses every frame.

If the scanner is not found:

- It must be on and connected. Press **Look again**.
- On Windows, check the driver (above).
- Quit other programs that may hold the scanner.
- If the app says the scanner program is missing or is not version 0.17.0, install that version (above).

A rare problem seen once: some twenty minutes after a roll preview that was stopped, the scanner reported no film and the film had to be put back by hand.

## 5. Camera capture with the Scanlight and the Nikon D600

With this method the app lights your film with the Scanlight V4 one colour at a time (red, then green, then blue), takes a picture of each with the camera, and joins the three into one frame. This was tried with a Nikon D600, a 60 mm lens and a Scanlight V4 rev A. Other cameras and lights are not tested. Steps that have not been tried on the real equipment are marked "not yet tried".

### macOS

This is the supported way to capture in version 0.1.0. The macOS app talks to the camera over USB with the part of the app that handles cameras, so there is nothing to install. The camera is fired over USB, so you do not need a shutter cable.

**Before you start**

1. Connect the Scanlight to the Mac by USB. If you also use the Scanlight's web page in Chrome, close that tab first, because the web page holds the connection.
2. Set the D600 to **M mode**, **manual focus**, **RAW**, with the lens cap off. Switch it on and connect it to the Mac by USB. The app's **Set up** page shows a checklist of some of these.
3. Leave the Scanlight's shutter jack empty: unplug the release cable from the light. On this type of light (rev A) the jack is not separated from the USB connection, and the Scanlight's own notes advise against joining the camera and the light to one computer through the release cable. The camera is fired over USB anyway. The app's checklist has a line "The release cable is connected." That line does not match the tested set-up; follow this guide.
4. Log out any other user accounts on the Mac and quit **Photos** and **Image Capture**. If macOS holds the camera, the app says so: do this, then unplug the camera and plug it in again.
5. Check **Settings** that **Guided capture** is switched on (it is by default).
6. Keep the camera and the film holder still for the whole roll. The app does not line the three pictures up, so a moved camera blurs the colours.

**The steps** (Import, choose your roll, then **Camera capture**)

1. **Set up.** Press **Connect light**. The app should show the light with its temperature and name the camera ("Camera: …"). Check ISO, shutter speed and aperture. The camera's own dials are locked while it is connected to the app, so you set them here. With a lens that has no electronic contacts, the aperture shows "Set on the lens".
2. **Focus and framing.** Put a film frame in the carrier. Press **Start live view** and choose a colour in the **Light** choices (live view does not turn the light on by itself; green is usually sharpest). Frame the picture. For fine focus choose a **Zoom** step (25 %, 50 %, 100 % or 200 %); **Focus peaking** and the **Sharpness** number help. A **Focus check** takes one real picture and shows its middle at full size. Nothing is stored.
3. **Flat field.** Take the film out, leave everything else as it is, and press **Capture flat field**. The bare light is photographed so the app can correct the light's unevenness in every frame. You can choose **Skip the flat field**; then frames are not corrected. This step is not yet tried on the real rig.
4. **Light levels.** Put the film back. The three LEDs need very different levels, and different film needs different levels. Frame clear film (the leader or the rebate) and press **Calibrate**. The app tries each colour until the brightest part is just short of clipping. You can draw an area on the live view to measure only that. Check with a **Test shot** for each colour, then press **Save for** your film stock or **Save as default**. A save needs a test shot at those levels. The app uses the saved levels for the next roll of the same film stock.
5. **Capture.** Press **Next frame** (or the Space bar). The app lights red, green and blue in turn and takes a picture each time. Advance the film by hand and press **Next frame** again. Press **Finish** when the roll is done.

The steps tried on the author's camera: the light, the camera, live view, focus aids, test shots, saved levels, automatic calibration and capture. The flat field and the guided step-by-step Import pages are not yet tried on the real hardware.

If the light gets too hot the app switches it off ("Light too hot and switched off"). Let it cool, then press **Reconnect light**.

### Windows

Camera capture is **not part of version 0.1.0 on Windows**. The Scanlight did not answer on the author's Windows computer, and live view and the camera link are on macOS only for now.

What you can do on Windows: if you can take three pictures of a frame lit red, green and blue by some other means, you can import them. In **Import** choose **Files**, then **Camera files (RGB triplets)** and **Choose camera files…**. The app groups files by capture time into triplets. Check the order of each one (red, green, blue) with **Earlier** and **Later**, confirm the ones to import, and start the import on the **Import** page. The import of camera files was tried on macOS; it has not been tried on Windows, and the author has no working way yet to light the film on Windows.

## 6. Known limitations

- The installers are not signed. Expect the warnings in [section 1](#1-install).
- Intel Macs are not supported. Windows on ARM is untested and unsupported (the Windows installer may run there through emulation).
- `nkscan` is installed by you and must be version 0.17.0 (section 4).
- Windows scanning needs the WinUSB driver; VueScan's driver does not work with the app.
- Only the author's whole-roll LS-50 is targeted. Other scanners are not supported.
- If the scanner's own detection puts a frame in the wrong place, you cannot yet move it: choosing frames only selects them (moving a frame is still being researched).
- Camera capture works on macOS only. The camera must stay fixed during a roll, because the three pictures are not lined up.
- The light must be kept cool. The app switches it off at a high temperature.
- A hung camera: if the camera stops answering, the app stops using it so the light stays safe. Switch the camera off and on, unplug and plug in its USB cable, press **Look for camera again**, and restart the app if that does not help.
- If the app is killed or the computer loses power while the Scanlight is lit, the light stays on. Switch it off with its own power switch. The app switches the light off when you leave the capture view, close the window or quit.
- Not planned: masking and local adjustments, AI generation, and advanced retouching. There is no cloud sync, no printing and no Linux or mobile version.

## 7. Troubleshooting

| What you see | What to do |
|---|---|
| Windows says "Windows protected your PC" | **More info**, then **Run anyway** ([section 1](#1-install)). |
| macOS says it cannot verify the app | **System Settings > Privacy & Security > Open Anyway** ([section 1](#1-install)). |
| "Set up your library" appears again | The app could not find your library. Choose **Open an existing library** and pick the folder ([section 3](#3-back-up-your-library)). |
| "This library was made by a newer version of the app" | Update the app. Meanwhile the library can be looked at, not changed. |
| "Verify library" lists a missing or changed original | Restore that file from your backup ([section 3](#3-back-up-your-library)). |
| "No scanner was found" | Check power and USB, press **Look again**, check the driver ([section 4](#4-the-nikon-coolscan-ls-50)). |
| The scanner is listed but "incompatible driver" or "held by another driver" (Windows) | Switch it to WinUSB ([section 4](#4-the-nikon-coolscan-ls-50)). |
| "The scanner program … could not be started" or "is not nkscan 0.17.0" | Install `nkscan` 0.17.0 ([section 4](#4-the-nikon-coolscan-ls-50)). |
| "No camera found" | Switch the camera on, check the USB cable, press **Look for camera again**. |
| A message about logging out other accounts and quitting Photos and Image Capture | Do that, then unplug and plug in the camera ([section 5](#5-camera-capture-with-the-scanlight-and-the-nikon-d600)). |
| "The camera stopped answering earlier…" | Switch the camera off and on, replug the cable, press **Look for camera again**, restart the app if needed. |
| "Light too hot and switched off" | Let it cool, then press **Reconnect light**. |
| "The USB supply of the light is low" | Use another USB port or a powered hub. |
| A test shot or capture is "clipped" | Lower that colour's level, or press **Calibrate** again. |
| "The three pictures used different shutter times" (or ISO, or aperture) | Set the camera to manual exposure and a fixed ISO. |
| "The camera settings differ from when these levels were saved" | Put the camera settings back, or calibrate again and save. |
| Camera capture on Windows | Not in version 0.1.0 ([section 5](#windows)). Import the three pictures as camera files. |

More technical notes are in [research/nkscan.md](research/nkscan.md) (scanner) and [research/scanlight-capture.md](research/scanlight-capture.md) (light and camera). The full design is in [specification.md](specification.md).

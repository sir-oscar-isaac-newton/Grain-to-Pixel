// The only place that talks to the Tauri backend. It wraps the commands generated from Rust
// (packages/ipc-types) and turns the `{ status: 'error' }` results into thrown `IpcError`s.
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog';
import {
  events,
  commands,
  type AppError,
  type CalibrationResult,
  type CalibrationWarning,
  type CameraInfo,
  type ZoomStep,
  type CameraSetting_Serialize,
  type CameraSettingKind,
  type CaptureLevels_Serialize,
  type CaptureProfile_Serialize,
  type CaptureProfilePatch_Deserialize,
  type CaptureStep,
  type ClientLogLevel,
  type Counters,
  type CrashNote,
  type DeviceInfo,
  type DevelopPreset_Serialize,
  type ExportedFile,
  type ExportOptions_Serialize,
  type ExportPreset_Serialize,
  type ExposureInfo_Serialize,
  type FocusCheck_Serialize,
  type FocusPattern,
  type FailedExport,
  type FilmFormat,
  type FlatFieldInfo,
  type FrameFlag,
  type ImportResult,
  type ImportWarning,
  type LastExport_Serialize,
  type LevelsScope,
  type LibraryDetails,
  type LibraryInfo,
  type LightInfo,
  type LightStatus_Serialize,
  type Look,
  type PortInfo,
  type Recipe,
  type RollPreviewImage,
  type ScanRequest,
  type Settings_Serialize,
  type SettingsPatch,
  type SkipReason,
  type SkippedFile,
  type TestShot_Serialize,
  type TripletCandidate,
  type TripletChannel,
  type TripletFile,
  type TripletProblem,
  type UpdateOutcome,
  type VerifyProblem,
  type VerifyReason,
  type VerifyResult,
} from '@gtp/ipc-types';

export type {
  AppError,
  CalibrationResult,
  CalibrationWarning,
  CameraInfo,
  ZoomStep,
  CameraSettingKind,
  CaptureStep,
  ClientLogLevel,
  Counters,
  CrashNote,
  DeviceInfo,
  ExportedFile,
  FailedExport,
  FilmFormat,
  FlatFieldInfo,
  FrameFlag,
  ImportResult,
  ImportWarning,
  LevelsScope,
  LibraryDetails,
  LibraryInfo,
  LightInfo,
  Look,
  PortInfo,
  RollPreviewImage,
  ScanRequest,
  SettingsPatch,
  SkipReason,
  SkippedFile,
  TripletCandidate,
  TripletChannel,
  TripletFile,
  TripletProblem,
  UpdateOutcome,
  VerifyProblem,
  VerifyReason,
  VerifyResult,
};

export class IpcError extends Error {
  readonly error: AppError;

  constructor(error: AppError) {
    super(error.message);
    this.name = 'IpcError';
    this.error = error;
  }
}

type Result<T> = { status: 'ok'; data: T } | { status: 'error'; error: AppError };

export function unwrap<T>(result: Result<T>): T {
  if (result.status === 'ok') return result.data;
  throw new IpcError(result.error);
}

export async function libraryInfo(): Promise<LibraryInfo> {
  return unwrap(await commands.libraryInfo());
}

/** Creates a library in an empty (or new) folder and opens it. */
export async function libraryCreate(path: string, name: string): Promise<LibraryInfo> {
  return unwrap(await commands.libraryCreate(path, name));
}

/** Opens a library; one made by a newer app version comes back with `read_only: true`. */
export async function libraryOpen(path: string): Promise<LibraryInfo> {
  return unwrap(await commands.libraryOpen(path));
}

/**
 * Starts verifying every original of the open library; returns the job id at once. Progress and
 * the `VerifyResult` arrive as `job://*` events, and `jobCancel` stops it. It only reports.
 */
export async function libraryVerify(): Promise<Data<'libraryVerify'>> {
  return unwrap(await commands.libraryVerify());
}

export async function settingsGet(): Promise<Settings> {
  return unwrap(await commands.settingsGet());
}

/** The settings as they are before anyone changes them (the defaults of the feature flags). */
export async function settingsDefaults(): Promise<Settings> {
  return commands.settingsDefaults();
}

/** Changes the named settings and returns all of them. */
export async function settingsSet(patch: SettingsPatch): Promise<Settings> {
  return unwrap(await commands.settingsSet(patch));
}

/**
 * Asks whether a newer version exists. `manual` is true for the button, which ignores the limit of
 * one check a day; nothing is asked when the update check is switched off.
 */
export async function updateCheck(manual: boolean): Promise<UpdateOutcome> {
  return unwrap(await commands.updateCheck(manual));
}

/** Downloads the update the last check found, verifies its signature and installs it. */
export async function updateInstall(): Promise<void> {
  unwrap(await commands.updateInstall());
}

/** Starts the app again to use the version that was installed. */
export async function updateRestart(): Promise<void> {
  await commands.updateRestart();
}

// Rolls and frames. Argument and result types come straight from the generated commands.
type Args<K extends keyof typeof commands> = Parameters<(typeof commands)[K]>;
type Data<K extends keyof typeof commands> =
  Awaited<ReturnType<(typeof commands)[K]>> extends Result<infer T> ? T : never;

export type RollInput = Args<'rollCreate'>[0];
export type FrameQuery = Args<'frameList'>[0];
export type Roll = Data<'rollCreate'>;
export type Frame = Data<'frameGet'>;
export type FramePage = Data<'frameList'>;
export type DeleteReport = Data<'rollDelete'>;

export async function rollCreate(input: RollInput): Promise<Roll> {
  return unwrap(await commands.rollCreate(input));
}

/** Replaces every editable field of the roll; an empty field is cleared. */
export async function rollUpdate(...args: Args<'rollUpdate'>): Promise<Roll> {
  return unwrap(await commands.rollUpdate(...args));
}

export async function rollList(): Promise<Roll[]> {
  return unwrap(await commands.rollList());
}

/** Sets the roll's film base, or clears it with `null`. */
export async function rollSetFilmBase(...args: Args<'rollSetFilmBase'>): Promise<Roll> {
  return unwrap(await commands.rollSetFilmBase(...args));
}

/** Sets the size of one frame on the roll's captures by hand, or clears it with null. */
export async function rollSetFrameSize(...args: Args<'rollSetFrameSize'>): Promise<Roll> {
  return unwrap(await commands.rollSetFrameSize(...args));
}

/** Makes a roll's flat field from three bare-light exposures (any order) and attaches it. */
export async function flatFieldImport(...args: Args<'flatFieldImport'>): Promise<FlatFieldInfo> {
  return unwrap(await commands.flatFieldImport(...args));
}

/** Removes a roll's flat field; false when it had none. */
export async function flatFieldClear(...args: Args<'flatFieldClear'>): Promise<boolean> {
  return unwrap(await commands.flatFieldClear(...args));
}

/** Deletes a roll and its frames; `files_left` lists files that could not be removed. */
export async function rollDelete(...args: Args<'rollDelete'>): Promise<DeleteReport> {
  return unwrap(await commands.rollDelete(...args));
}

export async function frameList(query: FrameQuery): Promise<FramePage> {
  return unwrap(await commands.frameList(query));
}

export async function frameGet(...args: Args<'frameGet'>): Promise<Frame> {
  return unwrap(await commands.frameGet(...args));
}

export async function frameSetNote(...args: Args<'frameSetNote'>): Promise<Frame> {
  return unwrap(await commands.frameSetNote(...args));
}

/** Sets the pick or reject mark of frames and returns the changed frames (unknown ids are skipped). */
export async function frameSetFlag(...args: Args<'frameSetFlag'>): Promise<FrameSummary[]> {
  return unwrap(await commands.frameSetFlag(...args));
}

/** Deletes frames; `files_left` lists files that could not be removed. */
export async function frameDelete(...args: Args<'frameDelete'>): Promise<DeleteReport> {
  return unwrap(await commands.frameDelete(...args));
}

/**
 * Starts importing files into a roll; returns the job id at once. Progress and the
 * `ImportResult` arrive as `job://*` events, each new frame as `library://changed`.
 */
export async function importFiles(...args: Args<'importFiles'>): Promise<Data<'importFiles'>> {
  return unwrap(await commands.importFiles(...args));
}

/**
 * What to export: format, colour space, JPEG quality, destination folder, naming template and
 * whether to write metadata.
 */
export type ExportOptions = ExportOptions_Serialize;

/** What the app returns of its settings. */
export type Settings = Settings_Serialize;

/** A named set of export options kept in the settings. */
export type ExportPreset = ExportPreset_Serialize;

/** A named group of Develop settings (a recipe and the parts of it to copy) kept in the settings. */
export type DevelopPreset = Omit<DevelopPreset_Serialize, 'recipe'> & { recipe: Recipe };

/** The options and preset of the export started last, kept in the settings. */
export type LastExport = LastExport_Serialize;

/**
 * Starts exporting frames at full resolution; returns the job id at once. Progress and the
 * `Exported` result (the files written and the frames that failed) arrive as `job://*` events.
 * An existing file is never overwritten: the name gets a number.
 */
export async function exportStart(...args: Args<'exportStart'>): Promise<Data<'exportStart'>> {
  return unwrap(await commands.exportStart(...args));
}

// Tags and search.
export type SearchQuery = Args<'search'>[0];
export type Tag = Data<'tagRename'>;

/** All tags, in name order. */
export async function tagList(): Promise<Data<'tagList'>> {
  return unwrap(await commands.tagList());
}

/** The tags of a roll, in name order. */
export async function rollTags(...args: Args<'rollTags'>): Promise<Tag[]> {
  return unwrap(await commands.rollTags(...args));
}

/** The tags on any of the frames themselves (not from their rolls), each once, in name order. */
export async function framesOwnTags(...args: Args<'framesOwnTags'>): Promise<Tag[]> {
  return unwrap(await commands.framesOwnTags(...args));
}

/**
 * Adds the tags named in `add` (created when missing) to the frames and removes those named in
 * `remove`, all or nothing.
 */
export async function tagApply(...args: Args<'tagApply'>): Promise<void> {
  unwrap(await commands.tagApply(...args));
}

/** The same for a roll: its tags count for every frame on it, also frames added later. */
export async function tagApplyRoll(...args: Args<'tagApplyRoll'>): Promise<void> {
  unwrap(await commands.tagApplyRoll(...args));
}

export async function tagRename(...args: Args<'tagRename'>): Promise<Data<'tagRename'>> {
  return unwrap(await commands.tagRename(...args));
}

export async function tagDelete(...args: Args<'tagDelete'>): Promise<void> {
  unwrap(await commands.tagDelete(...args));
}

/**
 * One page of the frames matching `query` (every part optional, all must match): best text match
 * first when there is text, otherwise newest roll first. `page` counts from 0.
 */
export async function search(...args: Args<'search'>): Promise<FramePage> {
  return unwrap(await commands.search(...args));
}

/** Crashes of earlier runs; the backend hands them out once. */
export async function crashNotesTake(): Promise<CrashNote[]> {
  return unwrap(await commands.crashNotesTake());
}

/** Sends an event to the application log. Never throws: reporting an error must not cause one. */
export async function logClientEvent(level: ClientLogLevel, message: string): Promise<void> {
  try {
    await commands.logClientEvent(level, message.slice(0, MAX_CLIENT_MESSAGE));
  } catch {
    // No backend (tests, plain browser) or a failed call: there is nowhere else to report it.
  }
}

// Keeps a message under the backend limit (2000 characters) with room for the ellipsis.
const MAX_CLIENT_MESSAGE = 1990;

// Folder checks, thumbnails, pickers, job control and events for the Library view.
export type FolderCheck = Data<'folderCheck'>;
export type FrameSummary = FramePage['items'][number];

/** What the first-run dialog shows about a folder. Nothing is created. */
export async function folderCheck(path: string): Promise<FolderCheck> {
  return unwrap(await commands.folderCheck(path));
}

export type CameraSetting = CameraSetting_Serialize;
export type CaptureProfile = CaptureProfile_Serialize;
export type CaptureLevels = CaptureLevels_Serialize;
export type ExposureInfo = ExposureInfo_Serialize;
export type TestShot = TestShot_Serialize;
export type FocusCheck = FocusCheck_Serialize;
export type { FocusPattern };
/** A 3 x 3 map of sharpness: rows from the top, columns from the left, the best region 1. */
export type FocusMap = FocusCheck_Serialize['map'];
export type CaptureProfilePatch = CaptureProfilePatch_Deserialize;
export type LightStatus = LightStatus_Serialize;

/** The serial ports the Scanlight may be on. */
export async function lightList(): Promise<PortInfo[]> {
  return unwrap(await commands.lightList());
}

/** Connects the light on `port`, or finds it by probing the ports; also clears a temperature trip. */
export async function lightConnect(port: string | null): Promise<LightInfo> {
  return unwrap(await commands.lightConnect(port));
}

/** Lights red, green and blue at these levels (0 to 255). */
export async function lightSet(levels: [number, number, number]): Promise<void> {
  unwrap(await commands.lightSet(levels));
}

/** Switches the light off; always allowed. */
export async function lightOff(): Promise<void> {
  unwrap(await commands.lightOff());
}

/** The cameras that can be used now. */
export async function cameraList(): Promise<CameraInfo[]> {
  return unwrap(await commands.cameraList());
}

/**
 * The camera's ISO, shutter speed and aperture with the choices it allows. A setting the camera
 * lacks has no value and no choices. Rejects while a capture runs.
 */
export async function cameraSettingsGet(): Promise<CameraSetting[]> {
  return unwrap(await commands.cameraSettingsGet());
}

/** Sets one camera setting to one of its choices; returns the settings as the camera has them now. */
export async function cameraSettingsSet(
  kind: CameraSettingKind,
  value: string,
): Promise<CameraSetting[]> {
  return unwrap(await commands.cameraSettingsSet(kind, value));
}

export async function captureProfileGet(): Promise<CaptureProfile> {
  return unwrap(await commands.captureProfileGet());
}

/** Changes the given fields of the capture profile; a refused change rejects with the reason. */
export async function captureProfileSet(patch: CaptureProfilePatch): Promise<CaptureProfile> {
  return unwrap(await commands.captureProfileSet(patch));
}

/** The three LED levels (red, green, blue), 0 to 255 each. */
export type Levels = [number, number, number];

/** The levels the roll captures with: saved for its film stock, else the global default. */
export async function captureLevelsGet(rollId: string): Promise<CaptureLevels> {
  return unwrap(await commands.captureLevelsGet(rollId));
}

/**
 * Saves levels as the global default or for a film stock, with the camera settings of the test
 * shot they were balanced on. Returns the levels as they now resolve for that scope.
 */
export async function captureLevelsSave(
  scope: LevelsScope,
  levels: Levels,
  exposure: ExposureInfo | null,
): Promise<CaptureLevels> {
  return unwrap(await commands.captureLevelsSave(scope, levels, exposure));
}

/**
 * Lights one colour, takes one picture and returns a 1:1 crop of its middle, its sharpness number
 * and a 3 x 3 sharpness map; stores nothing. The light is off afterwards.
 */
export async function captureFocusCheck(
  channel: TripletChannel,
  levels: Levels,
): Promise<FocusCheck> {
  return unwrap(await commands.captureFocusCheck(channel, levels));
}

/** Lights one colour at its working level, takes a picture and returns its histogram; stores nothing. */
export async function captureTest(channel: TripletChannel, levels: Levels): Promise<TestShot> {
  return unwrap(await commands.captureTest(channel, levels));
}

/** Captures one frame into the roll at the working levels as a job; the frame arrives in `job://done`. Returns the job id. */
export async function captureTriplet(rollId: string, levels: Levels): Promise<string> {
  return unwrap(await commands.captureTriplet(rollId, levels));
}

/**
 * Finds the Scanlight level of each colour that fills the camera's range as a job, shooting the
 * whole frame, or only `region` (fractions of the frame) when given; the `CalibrationResult` arrives in `job://done` and nothing is saved. Returns the job id.
 */
export async function captureCalibrate(
  rollId: string,
  levels: Levels,
  region: { x: number; y: number; w: number; h: number } | null = null,
): Promise<string> {
  return unwrap(await commands.captureCalibrate(rollId, levels, region));
}

/** The scanners that can be used now; fails with the reason (not connected, wrong driver ...) when none. */
export async function scannerList(): Promise<DeviceInfo[]> {
  return unwrap(await commands.scannerList());
}

/** The frame number a scan of the roll should continue at: one after the last scanned (1 when none). */
export async function scanResumePoint(roll: string): Promise<number> {
  return unwrap(await commands.scanResumePoint(roll));
}

/** Scans the requested frames into the roll as a job; each frame is stored as soon as it is done. */
export async function scanStart(roll: string, request: ScanRequest): Promise<string> {
  return unwrap(await commands.scanStart(roll, request));
}

/** The whole roll as the scanner's thumbnail (a positive strip) with the frame count and pitch. */
export async function scanPreview(device: string): Promise<RollPreviewImage> {
  return unwrap(await commands.scanPreview(device));
}

/** Stops a running roll preview; false when none is running. */
export async function scanPreviewCancel(): Promise<boolean> {
  return commands.scanPreviewCancel();
}

/** Stops a running job; false when it is not running (already finished), which is harmless. */
export async function jobCancel(id: string): Promise<boolean> {
  return unwrap(await commands.jobCancel(id));
}

function isAppError(value: unknown): value is AppError {
  return (
    typeof value === 'object' &&
    value !== null &&
    'code' in value &&
    'message' in value &&
    typeof (value as { message: unknown }).message === 'string'
  );
}

/**
 * A frame's thumbnail as JPEG bytes. This command returns raw bytes instead of JSON, so it is not
 * in the generated bindings (see `ipc::RAW_COMMANDS` in the shell); errors still arrive as
 * `AppError`s.
 */
export async function frameThumbnail(id: string): Promise<ArrayBuffer> {
  return invokeRaw('frame_thumbnail', { id });
}

/**
 * One live view frame of the camera as JPEG bytes (640 by 424). The first call puts the camera in
 * live view. Raw-bytes command like `frameThumbnail`; rejects while a capture, test shot or
 * calibration runs, and when the camera has no live view.
 */
export async function cameraPreviewFrame(): Promise<ArrayBuffer> {
  return invokeRaw('camera_preview_frame', {});
}

/** Takes the camera out of live view; harmless when it is not in live view or busy. */
export async function cameraPreviewStop(): Promise<void> {
  unwrap(await commands.cameraPreviewStop());
}

/** The zoom steps of the live view, whole frame first; empty when the camera has no zoom. */
export async function cameraPreviewZoomSteps(): Promise<ZoomStep[]> {
  return unwrap(await commands.cameraPreviewZoomSteps());
}

/** Sets the live view zoom to the `value` of one of the steps (starts live view if needed). */
export async function cameraPreviewSetZoom(value: string): Promise<void> {
  unwrap(await commands.cameraPreviewSetZoom(value));
}

/** Moves the zoomed area to a point of the frame, as fractions (0 to 1) of its width and height. */
export async function cameraPreviewSetArea(x: number, y: number): Promise<void> {
  unwrap(await commands.cameraPreviewSetArea(x, y));
}

/** Calls a raw-bytes command by name; an `AppError` it returns becomes an `IpcError`. */
async function invokeRaw(command: string, args: Record<string, unknown>): Promise<ArrayBuffer> {
  try {
    return await invoke<ArrayBuffer>(command, args);
  } catch (error) {
    if (isAppError(error)) throw new IpcError(error);
    throw error;
  }
}

// Develop: opening a frame, analysing it and rendering previews of it.
export type DevelopInfo = Data<'developOpen'>;
export type DevelopAnalysis = Data<'developAnalyze'>;
export type { Recipe };

/**
 * Opens a frame for editing (the backend decodes its original into a 2048 px proxy and keeps the
 * last four) and returns its size, saved recipe and the roll's film base.
 */
export async function developOpen(...args: Args<'developOpen'>): Promise<DevelopInfo> {
  return unwrap(await commands.developOpen(...args));
}

/**
 * Finds the frame, film base and levels, and the recipe they make. With a base (three linear scan
 * values, such as one picked on the rebate) the levels are fitted to that base, instead of the
 * roll's or a measured one; 
ull leaves it to the backend.
 */
export async function developAnalyze(...args: Args<'developAnalyze'>): Promise<DevelopAnalysis> {
  return unwrap(await commands.developAnalyze(...args));
}

/** A rectangle in fractions of the whole picture (x, y, width, height, each 0 to 1). */
export type Region = Args<'developSampleBase'>[1];

/**
 * The film base from an area of the whole scan, as the per-channel median in linear scan values.
 * The area is in fractions of the whole scan as
 * it was scanned (the Develop view maps a pick on the turned and flipped picture back first).
 */
export async function developSampleBase(
  ...args: Args<'developSampleBase'>
): Promise<Data<'developSampleBase'>> {
  return unwrap(await commands.developSampleBase(...args));
}

/** Which end of the levels a picked area sets. */
export type LevelsPoint = Args<'developSampleLevels'>[3];

/**
 * The recipe's levels with the white or black point set from an area of the frame. The area is in
 * fractions of the cropped frame before straightening, turns and flips (the Develop view maps a
 * pick on the shown picture back first).
 */
export async function developSampleLevels(
  ...args: Args<'developSampleLevels'>
): Promise<Data<'developSampleLevels'>> {
  return unwrap(await commands.developSampleLevels(...args));
}

/**
 * The recipe's colour settings with the three colour axes set so that an area of the frame becomes
 * neutral. The area is in fractions of the cropped frame, as for {@link developSampleLevels}.
 */
export async function developSampleNeutral(
  ...args: Args<'developSampleNeutral'>
): Promise<Data<'developSampleNeutral'>> {
  return unwrap(await commands.developSampleNeutral(...args));
}

/** A group of settings that can be copied between frames (one panel of the Develop view). */
export type RecipeField = Args<'recipeApplyTo'>[2][number];

/**
 * Saves a frame's recipe and marks the frame edited. The backend clamps it into range and returns
 * the recipe as stored; a recipe it could not render (a tone curve with one point) is refused.
 */
export async function recipeSave(...args: Args<'recipeSave'>): Promise<Data<'recipeSave'>> {
  return unwrap(await commands.recipeSave(...args));
}

/**
 * Copies the chosen groups of settings from the `source` frame's recipe onto the frames, all or
 * nothing; whatever else a target has is kept. `Color` includes the black-and-white conversion.
 */
export async function recipeApplyTo(...args: Args<'recipeApplyTo'>): Promise<void> {
  unwrap(await commands.recipeApplyTo(...args));
}

/**
 * Copies the chosen groups of settings from a preset's `recipe` onto the frames, all or nothing,
 * like {@link recipeApplyTo} but with no source frame: every frame listed is a target.
 */
export async function recipeApplyValuesTo(...args: Args<'recipeApplyValuesTo'>): Promise<void> {
  unwrap(await commands.recipeApplyValuesTo(...args));
}

/**
 * The `target` recipe with the chosen groups of settings copied from `source`, clamped. Reads and
 * saves nothing; the Develop view applies a preset to the open edit with it.
 */
export async function recipeMergeFields(
  ...args: Args<'recipeMergeFields'>
): Promise<Data<'recipeMergeFields'>> {
  return unwrap(await commands.recipeMergeFields(...args));
}

/** A rendered preview: RGBA8 sRGB pixels, ready for `new ImageData(rgba, width, height)`. */
export interface Preview {
  /** The sequence number of the request this answers. */
  seq: number;
  width: number;
  height: number;
  rgba: Uint8ClampedArray;
}

const PREVIEW_HEADER_BYTES = 16;

/**
 * Reads a preview from the bytes of `develop_render`: a little-endian header (`seq` u64, `width`
 * u32, `height` u32) and then the RGBA8 pixels.
 */
export function parsePreview(buffer: ArrayBuffer): Preview {
  if (buffer.byteLength < PREVIEW_HEADER_BYTES) throw new Error('The preview is incomplete.');
  const header = new DataView(buffer);
  const seq = Number(header.getBigUint64(0, true));
  const width = header.getUint32(8, true);
  const height = header.getUint32(12, true);
  if (buffer.byteLength !== PREVIEW_HEADER_BYTES + width * height * 4) {
    throw new Error('The preview does not match its size.');
  }
  return {
    seq,
    width,
    height,
    rgba: new Uint8ClampedArray(buffer, PREVIEW_HEADER_BYTES, width * height * 4),
  };
}

/**
 * Renders a preview of the frame with `recipe`, at most `maxEdge` pixels on its long edge (256 to
 * 4096; the proxy is 2048). Numbers `seq` upwards: when a newer request for the same frame arrives
 * first, this one rejects with an `IpcError` whose code is `Cancelled`, which is not a failure.
 * Like `frameThumbnail`, it is a raw-bytes command and not in the generated bindings.
 */
export async function developRender(
  frameId: string,
  recipe: Recipe,
  maxEdge: number,
  seq: number,
): Promise<Preview> {
  return parsePreview(await invokeRaw('develop_render', { frameId, recipe, maxEdge, seq }));
}

/** The most pixels a detail region may have on a side (the backend refuses more). */
export const MAX_DETAIL_EDGE = 4096;

/** A part of the picture as fractions of the whole picture (each 0 to 1). */
export interface DetailRegion {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * A part of a frame rendered at full size: RGBA8 sRGB pixels, where the part is (in pixels of the
 * full picture) and how big the full picture is.
 */
export interface Detail {
  /** The sequence number of the request this answers. */
  seq: number;
  /** The size of the part. */
  width: number;
  height: number;
  /** Where the part starts in the full picture. */
  x: number;
  y: number;
  /** The size of the full picture. */
  fullWidth: number;
  fullHeight: number;
  rgba: Uint8ClampedArray;
}

const DETAIL_HEADER_BYTES = 32;

/**
 * Reads a detail region from the bytes of `develop_render_detail`: a little-endian header (`seq`
 * u64, region `width`, `height`, `x`, `y`, full picture `width`, `height`, each u32) and then the
 * RGBA8 pixels.
 */
export function parseDetail(buffer: ArrayBuffer): Detail {
  if (buffer.byteLength < DETAIL_HEADER_BYTES) throw new Error('The detail is incomplete.');
  const header = new DataView(buffer);
  const word = (i: number) => header.getUint32(8 + i * 4, true);
  const width = word(0);
  const height = word(1);
  if (buffer.byteLength !== DETAIL_HEADER_BYTES + width * height * 4) {
    throw new Error('The detail does not match its size.');
  }
  return {
    seq: Number(header.getBigUint64(0, true)),
    width,
    height,
    x: word(2),
    y: word(3),
    fullWidth: word(4),
    fullHeight: word(5),
    rgba: new Uint8ClampedArray(buffer, DETAIL_HEADER_BYTES, width * height * 4),
  };
}

/**
 * Renders the part `region` of the frame at full size with `recipe`, as a full-size export would
 * (output sharpening included). The first request for a recipe renders the whole picture, which
 * takes seconds; later regions of the same recipe are cut from it. Numbers `seq` upwards: a request
 * a newer one replaced rejects with an `IpcError` whose code is `Cancelled`, which is not a
 * failure. Raw-bytes command like `developRender`, so not in the generated bindings.
 */
export async function developRenderDetail(
  frameId: string,
  recipe: Recipe,
  region: DetailRegion,
  seq: number,
): Promise<Detail> {
  return parseDetail(await invokeRaw('develop_render_detail', { frameId, recipe, region, seq }));
}

/**
 * Releases the full-size picture the backend keeps for the detail view (about 4 bytes a pixel).
 * Called when the Develop view is left; harmless with nothing cached.
 */
export async function developReleaseDetail(): Promise<void> {
  unwrap(await commands.developReleaseDetail());
}

/** The activation counters of the open library; null when no library is open. */
export async function countersGet(): Promise<Counters | null> {
  return unwrap(await commands.countersGet());
}

/** Writes the diagnostics zip (logs, versions, flags, counters, last job errors) to `dest`. */
export async function diagnosticsExport(dest: string): Promise<void> {
  unwrap(await commands.diagnosticsExport(dest));
}

/** Opens the system save dialog for the diagnostics zip; null when cancelled. */
export async function pickDiagnosticsFile(): Promise<string | null> {
  const chosen = await saveDialog({
    defaultPath: 'grain-to-pixel-diagnostics.zip',
    filters: [{ name: 'Zip', extensions: ['zip'] }],
  });
  return chosen ?? null;
}

/** Opens the system folder picker; null when cancelled. */
export async function pickFolder(): Promise<string | null> {
  const chosen = await openDialog({ directory: true, multiple: false });
  return typeof chosen === 'string' ? chosen : null;
}

/** Groups camera files (NEF) into R/G/B triplet candidates by capture time. Reads EXIF only. */
export async function tripletGroup(paths: string[]): Promise<TripletCandidate[]> {
  return unwrap(await commands.tripletGroup(paths));
}

/** Opens the system file picker for camera files (NEF); an empty list when cancelled. */
export async function pickCameraFiles(): Promise<string[]> {
  const chosen = await openDialog({
    multiple: true,
    directory: false,
    filters: [{ name: 'Nikon NEF', extensions: ['nef'] }],
  });
  if (chosen === null) return [];
  return Array.isArray(chosen) ? chosen : [chosen];
}

/** Opens the system file picker for TIFF scans; an empty list when cancelled. */
export async function pickTiffFiles(): Promise<string[]> {
  const chosen = await openDialog({
    multiple: true,
    directory: false,
    filters: [{ name: 'TIFF', extensions: ['tif', 'tiff'] }],
  });
  if (chosen === null) return [];
  return Array.isArray(chosen) ? chosen : [chosen];
}

/**
 * Calls `cb` when frames were added, changed or removed. `frameIds` lists the frames whose
 * thumbnails were regenerated (their recipe changed), empty for any other change. Returns a
 * function that stops listening.
 */
export async function onLibraryChanged(
  cb: (rollId: string | null, frameIds: string[]) => void,
): Promise<() => void> {
  return events.libraryChanged.listen((event) =>
    cb(event.payload.roll_id ?? null, event.payload.frame_ids ?? []),
  );
}

export interface JobProgressEvent {
  id: string;
  stage: string;
  fraction: number;
}

export async function onJobProgress(cb: (e: JobProgressEvent) => void): Promise<() => void> {
  return events.jobProgress.listen((event) =>
    cb({ ...event.payload, fraction: event.payload.fraction ?? 0 }),
  );
}

type JobDoneEvent = Parameters<Parameters<typeof events.jobDone.listen>[0]>[0]['payload'];
type JobErrorEvent = Parameters<Parameters<typeof events.jobError.listen>[0]>[0]['payload'];
export type JobResult = JobDoneEvent['result'];

export async function onJobDone(cb: (e: JobDoneEvent) => void): Promise<() => void> {
  return events.jobDone.listen((event) => cb(event.payload));
}

export async function onJobError(cb: (e: JobErrorEvent) => void): Promise<() => void> {
  return events.jobError.listen((event) => cb(event.payload));
}

export async function onCaptureStep(cb: (e: CaptureStep) => void): Promise<() => void> {
  return events.captureStep.listen((event) => cb(event.payload));
}

export async function onLightStatus(cb: (e: LightStatus) => void): Promise<() => void> {
  return events.lightStatus.listen((event) => cb(event.payload));
}

import * as Dialog from '@radix-ui/react-dialog';
import { ProgressList, Slider } from '@gtp/ui';
import { useEffect, useId, useRef, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import {
  exportStart,
  frameList,
  jobCancel,
  pickFolder,
  settingsGet,
  settingsSet,
  type ExportOptions,
  type ExportPreset,
} from '../../shared/ipc';
import { describeJob } from './JobsPanel';
import { applyPreset, findPreset, isModified, nameProblem } from './export-presets';
import styles from './library.module.css';
import { messageOf, useLibraryStore } from './library-store';

type Format = ExportOptions['format'];
type Space = ExportOptions['space'];

const FORMATS: readonly Format[] = ['tiff16', 'jpeg'];
const SPACES: readonly Space[] = ['srgb', 'adobe_rgb', 'prophoto'];
const DEFAULT_TEMPLATE = '{roll}_{index:03}';

const DEFAULTS: ExportOptions = {
  format: 'tiff16',
  space: 'srgb',
  jpeg_quality: 92,
  dest_dir: '',
  template: DEFAULT_TEMPLATE,
  write_metadata: true,
};

/** What the file name would be for a roll "Roll", frame 1; null when the template is not valid. */
export function exampleName(template: string, format: Format): string | null {
  const extension = format === 'tiff16' ? 'tif' : 'jpg';
  const filled = template
    .replace(/\{roll\}/g, 'Roll')
    .replace(/\{index:0([1-9])\}/g, (_, width: string) => '1'.padStart(Number(width), '0'))
    .replace(/\{index\}/g, '1')
    .replace(/\{frame\}/g, '01J0000000000000000000000')
    .replace(/\{ext\}/g, extension);
  if (/[{}]/.test(filled) || filled.trim() === '') return null;
  return filled.toLowerCase().endsWith(`.${extension}`) ? filled : `${filled}.${extension}`;
}

/** The translation key of what is wrong with the options, or null. */
function problemOf(options: ExportOptions): string | null {
  if (options.dest_dir.trim() === '') return 'export.errors.noFolder';
  if (options.template.trim() === '') return 'export.errors.noTemplate';
  if (/[\\/]/.test(options.template)) return 'export.errors.folders';
  return null;
}

const PICKED_PAGE_SIZE = 500;
/** The picked frames are read in pages of {@link PICKED_PAGE_SIZE}; no real library needs more. */
const MAX_PICKED_PAGES = 20;

/** The ids of the frames in `frameIds` that are picked (read from the library's picked frames). */
async function pickedAmong(frameIds: readonly string[]): Promise<string[]> {
  const wanted = new Set(frameIds);
  const found = new Set<string>();
  let seen = 0;
  for (let page = 0; page < MAX_PICKED_PAGES; page += 1) {
    const result = await frameList({
      roll_id: null,
      flag: 'pick',
      page,
      page_size: PICKED_PAGE_SIZE,
    });
    for (const frame of result.items) if (wanted.has(frame.id)) found.add(frame.id);
    seen += result.items.length;
    if (seen >= result.total || result.items.length === 0) break;
  }
  return frameIds.filter((id) => found.has(id));
}

interface ExportDialogProps {
  /** The frames to export. */
  frameIds: readonly string[];
  open: boolean;
  onClose: () => void;
}

/**
 * The export dialog: format, colour space, quality, destination folder and file name, then the
 * progress of the job, a summary of what was written and what failed, and cancel. Closing it does
 * not stop the job; reopening it shows the export that is running.
 */
export function ExportDialog({ frameIds, open, onClose }: ExportDialogProps) {
  const { t, i18n } = useTranslation();
  const addJob = useLibraryStore((s) => s.addJob);
  const dismissJob = useLibraryStore((s) => s.dismissJob);
  const running = useLibraryStore((s) =>
    s.jobs.find((j) => j.kind === 'export' && j.state === 'running'),
  );

  const [options, setOptions] = useState<ExportOptions>(DEFAULTS);
  const [jobId, setJobId] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const job = useLibraryStore((s) => s.jobs.find((j) => j.id === jobId));

  // Which of the frames are picked, found once when the dialog opens (null: not known yet, or
  // not readable; the option is then not offered). The option starts off.
  const [picked, setPicked] = useState<string[] | null>(null);
  const [onlyPicked, setOnlyPicked] = useState(false);
  // The parent passes a new array on every render, so the effect follows the ids' content.
  const idsKey = frameIds.join(',');
  useEffect(() => {
    let alive = true;
    pickedAmong(idsKey === '' ? [] : idsKey.split(',')).then(
      (ids) => alive && setPicked(ids),
      () => {
        // Without the flags the dialog exports every frame, as before.
      },
    );
    return () => {
      alive = false;
    };
  }, [idsKey]);
  const offerPicked = picked !== null && picked.length > 0;
  const limited = offerPicked && onlyPicked;
  const exported = limited ? picked : frameIds;

  // Opening the dialog while an export runs shows that export.
  useEffect(() => {
    if (open && jobId === null && running) setJobId(running.id);
  }, [open, jobId, running]);

  // The presets and the options used last come from the settings when the dialog opens. What the
  // user has already changed by then is not overwritten.
  const [presets, setPresets] = useState<ExportPreset[]>([]);
  const [presetName, setPresetName] = useState<string | null>(null);
  const [presetError, setPresetError] = useState<string | null>(null);
  const [naming, setNaming] = useState<string | null>(null);
  const [nameError, setNameError] = useState<string | null>(null);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const touched = useRef(false);
  const nameRef = useRef<HTMLInputElement>(null);
  const namingOpen = naming !== null;
  useEffect(() => {
    if (namingOpen) nameRef.current?.focus();
  }, [namingOpen]);
  const nameId = useId();
  const nameErrorId = useId();
  useEffect(() => {
    let alive = true;
    settingsGet().then(
      (settings) => {
        if (!alive) return;
        const list = settings.export_presets ?? [];
        setPresets(list);
        const last = settings.last_export;
        if (last && !touched.current) {
          setOptions(last.options);
          setPresetName(findPreset(list, last.preset)?.name ?? null);
        }
      },
      (e: unknown) =>
        alive && setPresetError(t('export.presets.loadFailed', { message: messageOf(e) })),
    );
    return () => {
      alive = false;
    };
  }, [t]);

  const change = (patch: Partial<ExportOptions>) => {
    touched.current = true;
    setOptions((o) => ({ ...o, ...patch }));
  };
  const chosenPreset = findPreset(presets, presetName);
  const modified = chosenPreset !== undefined && isModified(options, chosenPreset);

  const choosePreset = (name: string) => {
    touched.current = true;
    setPresetError(null);
    setConfirmingDelete(false);
    const preset = findPreset(presets, name);
    setPresetName(preset?.name ?? null);
    if (preset) setOptions((o) => applyPreset(o, preset));
  };

  /** Writes the whole list of presets; true when it was saved (the list shown is then the saved one). */
  const writePresets = async (list: ExportPreset[]): Promise<boolean> => {
    setPresetError(null);
    try {
      const saved = await settingsSet({ export_presets: list });
      setPresets(saved.export_presets ?? list);
      return true;
    } catch (e) {
      setPresetError(t('export.presets.failed', { message: messageOf(e) }));
      return false;
    }
  };
  const shown = (): ExportOptions => ({ ...options, template: options.template.trim() });

  const saveAs = async () => {
    const name = (naming ?? '').trim();
    const problem = nameProblem(name, presets);
    if (problem !== null) {
      setNameError(t(problem));
      return;
    }
    if (await writePresets([...presets, { name, options: shown() }])) {
      setPresetName(name);
      setNaming(null);
      setNameError(null);
    }
  };
  const update = async () => {
    if (!chosenPreset) return;
    await writePresets(
      presets.map((p) => (p === chosenPreset ? { name: p.name, options: shown() } : p)),
    );
  };
  const remove = async () => {
    if (!chosenPreset) return;
    if (await writePresets(presets.filter((p) => p !== chosenPreset))) {
      setPresetName(null);
      setConfirmingDelete(false);
    }
  };
  const problem = problemOf(options);
  const folderProblem = problem === 'export.errors.noFolder';
  const example = exampleName(options.template, options.format);

  const choose = async () => {
    setError(null);
    try {
      const picked = await pickFolder();
      if (picked) change({ dest_dir: picked });
    } catch (e) {
      setError(messageOf(e));
    }
  };

  const start = async (event: FormEvent) => {
    event.preventDefault();
    if (problem !== null || starting) return;
    setStarting(true);
    setError(null);
    try {
      const chosen = { ...options, template: options.template.trim() };
      const id = await exportStart([...exported], chosen);
      try {
        await settingsSet({ last_export: { options: chosen, preset: chosenPreset?.name ?? null } });
      } catch {
        // Not remembered for next time: the export itself is running.
      }
      addJob({
        id,
        kind: 'export',
        rollName: '',
        frames: exported.length,
        destination: chosen.dest_dir,
        fraction: 0,
        state: 'running',
      });
      setJobId(id);
    } catch (e) {
      setError(t('export.startFailed', { message: messageOf(e) }));
    } finally {
      setStarting(false);
    }
  };

  const finished = job !== undefined && job.state !== 'running';

  return (
    <Dialog.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <Dialog.Content className={styles.dialog}>
          <Dialog.Title>{t('export.title')}</Dialog.Title>
          <Dialog.Description>
            {limited && !job
              ? t('export.descriptionPicked', { count: exported.length })
              : t('export.description', { count: job?.frames ?? exported.length })}
          </Dialog.Description>

          {job ? (
            <div className={styles.stack}>
              <ProgressList
                items={[describeJob(job, t, i18n.language)]}
                label={t('jobs.label')}
                cancelLabel={t('jobs.cancel')}
                dismissLabel={t('jobs.dismiss')}
                onCancel={(id) => void jobCancel(id)}
                onDismiss={(id) => {
                  dismissJob(id);
                  setJobId(null);
                }}
              />
              {job.state === 'done' && job.exported && (
                <p role="status">
                  {t('export.summary', {
                    count: job.exported.length,
                    folder: job.destination ?? '',
                  })}
                </p>
              )}
              {job.state === 'running' && <p className={styles.hint}>{t('export.runningHint')}</p>}
              <div className={styles.buttons}>
                {finished && (
                  <button type="button" onClick={() => setJobId(null)}>
                    {t('export.again')}
                  </button>
                )}
                <button type="button" onClick={onClose}>
                  {t('export.close')}
                </button>
              </div>
            </div>
          ) : (
            <form onSubmit={start} className={styles.form}>
              <label className={styles.field}>
                {t('export.presets.label')}
                <select
                  value={chosenPreset?.name ?? ''}
                  onChange={(e) => choosePreset(e.target.value)}
                >
                  <option value="">{t('export.presets.none')}</option>
                  {presets.map((p) => (
                    <option key={p.name} value={p.name}>
                      {p === chosenPreset && modified
                        ? t('export.presets.modified', { name: p.name })
                        : p.name}
                    </option>
                  ))}
                </select>
              </label>
              <div className={styles.buttons}>
                <button type="button" onClick={() => setNaming('')} disabled={naming !== null}>
                  {t('export.presets.saveAs')}
                </button>
                <button type="button" onClick={() => void update()} disabled={!modified}>
                  {t('export.presets.update')}
                </button>
                <button
                  type="button"
                  onClick={() => setConfirmingDelete(true)}
                  disabled={!chosenPreset || confirmingDelete}
                >
                  {t('export.presets.delete')}
                </button>
              </div>
              {naming !== null && (
                <div className={styles.field}>
                  <label htmlFor={nameId}>{t('export.presets.name')}</label>
                  <input
                    id={nameId}
                    type="text"
                    value={naming}
                    maxLength={100}
                    ref={nameRef}
                    aria-invalid={nameError !== null}
                    aria-describedby={nameError ? nameErrorId : undefined}
                    onChange={(e) => {
                      setNaming(e.target.value);
                      setNameError(null);
                    }}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') {
                        e.preventDefault();
                        void saveAs();
                      }
                    }}
                  />
                  {nameError && (
                    <p id={nameErrorId} role="alert">
                      {nameError}
                    </p>
                  )}
                  <div className={styles.buttons}>
                    <button type="button" onClick={() => void saveAs()}>
                      {t('export.presets.save')}
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setNaming(null);
                        setNameError(null);
                      }}
                    >
                      {t('export.presets.cancel')}
                    </button>
                  </div>
                </div>
              )}
              {confirmingDelete && chosenPreset && (
                <div role="group" aria-label={t('export.presets.confirmLabel')}>
                  <p>{t('export.presets.confirmDelete', { name: chosenPreset.name })}</p>
                  <div className={styles.buttons}>
                    <button type="button" onClick={() => void remove()}>
                      {t('export.presets.confirm')}
                    </button>
                    <button type="button" onClick={() => setConfirmingDelete(false)}>
                      {t('export.presets.keep')}
                    </button>
                  </div>
                </div>
              )}
              {presetError && <p role="alert">{presetError}</p>}

              <fieldset className={styles.fieldset}>
                <legend>{t('export.format')}</legend>
                {FORMATS.map((format) => (
                  <label key={format} className={styles.radio}>
                    <input
                      type="radio"
                      name="format"
                      value={format}
                      checked={options.format === format}
                      onChange={() => change({ format })}
                    />
                    {t(`export.${format}`)}
                    <span className={styles.hint}>{t(`export.${format}Hint`)}</span>
                  </label>
                ))}
              </fieldset>

              <label className={styles.field}>
                {t('export.space')}
                <select
                  value={options.space}
                  onChange={(e) => change({ space: e.target.value as Space })}
                >
                  {SPACES.map((space) => (
                    <option key={space} value={space}>
                      {t(`export.${space}`)}
                    </option>
                  ))}
                </select>
                <span className={styles.hint}>{t(`export.spaceHint.${options.space}`)}</span>
              </label>

              {options.format === 'jpeg' && (
                <Slider
                  label={t('export.quality')}
                  value={options.jpeg_quality}
                  min={1}
                  max={100}
                  step={1}
                  onChange={(jpeg_quality) => change({ jpeg_quality })}
                />
              )}

              {offerPicked && (
                <label className={styles.radio}>
                  <input
                    type="checkbox"
                    checked={onlyPicked}
                    onChange={(e) => setOnlyPicked(e.target.checked)}
                  />
                  {t('export.onlyPicked')}
                </label>
              )}

              <div className={styles.folderRow}>
                <span>{t('export.folder')}</span>
                <span className={styles.path}>{options.dest_dir || t('export.noFolder')}</span>
                <button type="button" onClick={() => void choose()}>
                  {t('export.choose')}
                </button>
              </div>

              <label className={styles.field}>
                {t('export.template')}
                <input
                  type="text"
                  value={options.template}
                  maxLength={200}
                  spellCheck={false}
                  aria-invalid={problem !== null && !folderProblem}
                  onChange={(e) => change({ template: e.target.value })}
                />
                <span className={styles.hint}>{t('export.templateHint')}</span>
                {example && (
                  <span className={styles.hint}>{t('export.example', { name: example })}</span>
                )}
              </label>

              <div className={styles.field}>
                <label className={styles.radio}>
                  <input
                    type="checkbox"
                    checked={options.write_metadata}
                    aria-describedby="export-metadata-hint"
                    onChange={(e) => change({ write_metadata: e.target.checked })}
                  />
                  {t('export.writeMetadata')}
                </label>
                <span id="export-metadata-hint" className={styles.hint}>
                  {t('export.writeMetadataHint')}
                </span>
              </div>

              {problem !== null && !folderProblem && <p role="alert">{t(problem)}</p>}
              {error && <p role="alert">{error}</p>}
              <div className={styles.buttons}>
                <button type="submit" disabled={problem !== null || starting}>
                  {starting ? t('export.starting') : t('export.start')}
                </button>
                <button type="button" onClick={onClose}>
                  {t('export.close')}
                </button>
              </div>
            </form>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

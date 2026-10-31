import * as Dialog from '@radix-ui/react-dialog';
import { useId, useMemo, useState, type FormEvent, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { rollCreate, rollUpdate, type Roll, type RollInput } from '../../shared/ipc';
import styles from './library.module.css';
import { messageOf, useLibraryStore } from './library-store';

interface RollEditorProps {
  /** The roll to edit, or null to create one. */
  roll: Roll | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

type Errors = Partial<Record<'name' | 'isoBox' | 'isoShot', string>>;

interface ControlProps {
  id: string;
  'aria-invalid'?: true;
  'aria-describedby'?: string;
}

/** A labelled control with its error message beside it (not inside the label). */
function Field({
  label,
  error,
  describedBy,
  children,
}: {
  label: string;
  error?: string;
  describedBy?: string;
  children: (props: ControlProps) => ReactNode;
}) {
  const id = useId();
  const errorId = `${id}-error`;
  const described = error ? errorId : describedBy;
  return (
    <div className={styles.field}>
      <label htmlFor={id}>{label}</label>
      {children({
        id,
        'aria-invalid': error ? true : undefined,
        'aria-describedby': described,
      })}
      {error && (
        <span id={errorId} role="alert">
          {error}
        </span>
      )}
    </div>
  );
}

const text = (value: string): string | null => value.trim() || null;

/** A whole number above 0, or null for empty; `undefined` when the text is not such a number. */
function iso(value: string): number | null | undefined {
  const v = value.trim();
  if (v === '') return null;
  if (!/^\d+$/.test(v)) return undefined;
  const n = Number(v);
  return n > 0 ? n : undefined;
}

/**
 * The roll form: name, film, camera, box and shot ISO, the shooting dates and notes. Camera and
 * film suggest what earlier rolls used. The backend checks the dates and ISO range too, and its
 * message is shown if it refuses.
 */
export function RollEditor({ roll, open, onOpenChange }: RollEditorProps) {
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <Dialog.Content className={styles.dialog}>
          {/* Remounted per roll, so the fields always start from that roll. */}
          <RollForm key={roll?.id ?? 'new'} roll={roll} onDone={() => onOpenChange(false)} />
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function RollForm({ roll, onDone }: { roll: Roll | null; onDone: () => void }) {
  const { t } = useTranslation();
  const rolls = useLibraryStore((s) => s.rolls);
  const reloadRolls = useLibraryStore((s) => s.reloadRolls);
  const selectRoll = useLibraryStore((s) => s.selectRoll);
  const ids = { film: useId(), camera: useId(), dates: useId() };

  const [name, setName] = useState(roll?.name ?? '');
  const [film, setFilm] = useState(roll?.film_stock ?? '');
  const [camera, setCamera] = useState(roll?.camera ?? '');
  const [isoBox, setIsoBox] = useState(roll?.iso_box?.toString() ?? '');
  const [isoShot, setIsoShot] = useState(roll?.iso_shot?.toString() ?? '');
  const [from, setFrom] = useState(roll?.shot_from ?? '');
  const [to, setTo] = useState(roll?.shot_to ?? '');
  const [notes, setNotes] = useState(roll?.notes ?? '');
  const [errors, setErrors] = useState<Errors>({});
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const earlier = useMemo(() => {
    const distinct = (pick: (r: Roll) => string | null) =>
      [...new Set(rolls.map(pick).filter((v): v is string => !!v))].sort();
    return { film: distinct((r) => r.film_stock), camera: distinct((r) => r.camera) };
  }, [rolls]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const box = iso(isoBox);
    const shot = iso(isoShot);
    const found: Errors = {};
    if (!name.trim()) found.name = t('rollEditor.nameRequired');
    if (box === undefined) found.isoBox = t('rollEditor.isoInvalid');
    if (shot === undefined) found.isoShot = t('rollEditor.isoInvalid');
    setErrors(found);
    if (Object.keys(found).length > 0) return;

    const input: RollInput = {
      name: name.trim(),
      film_stock: text(film),
      camera: text(camera),
      iso_box: box ?? null,
      iso_shot: shot ?? null,
      shot_from: text(from),
      shot_to: text(to),
      notes: notes.trim(),
    };
    setBusy(true);
    setFailure(null);
    try {
      const saved = roll ? await rollUpdate(roll.id, input) : await rollCreate(input);
      await reloadRolls();
      selectRoll(saved.id);
      onDone();
    } catch (e) {
      setFailure(messageOf(e));
      setBusy(false);
    }
  };

  return (
    <>
      <Dialog.Title>{roll ? t('rollEditor.titleEdit') : t('rollEditor.titleNew')}</Dialog.Title>
      <Dialog.Description>{t('rollEditor.description')}</Dialog.Description>
      <form onSubmit={submit} className={styles.form} noValidate>
        <Field label={t('rollEditor.name')} error={errors.name}>
          {(props) => (
            <input
              {...props}
              type="text"
              value={name}
              maxLength={200}
              onChange={(e) => setName(e.target.value)}
            />
          )}
        </Field>
        <Field label={t('rollEditor.film')}>
          {(props) => (
            <>
              <input
                {...props}
                type="text"
                list={ids.film}
                value={film}
                onChange={(e) => setFilm(e.target.value)}
              />
              <datalist id={ids.film}>
                {earlier.film.map((v) => (
                  <option key={v} value={v} />
                ))}
              </datalist>
            </>
          )}
        </Field>
        <Field label={t('rollEditor.camera')}>
          {(props) => (
            <>
              <input
                {...props}
                type="text"
                list={ids.camera}
                value={camera}
                onChange={(e) => setCamera(e.target.value)}
              />
              <datalist id={ids.camera}>
                {earlier.camera.map((v) => (
                  <option key={v} value={v} />
                ))}
              </datalist>
            </>
          )}
        </Field>
        <div className={styles.pair}>
          <Field label={t('rollEditor.isoBox')} error={errors.isoBox}>
            {(props) => (
              <input
                {...props}
                type="text"
                inputMode="numeric"
                value={isoBox}
                onChange={(e) => setIsoBox(e.target.value)}
              />
            )}
          </Field>
          <Field label={t('rollEditor.isoShot')} error={errors.isoShot}>
            {(props) => (
              <input
                {...props}
                type="text"
                inputMode="numeric"
                value={isoShot}
                onChange={(e) => setIsoShot(e.target.value)}
              />
            )}
          </Field>
        </div>
        <div className={styles.pair}>
          <Field label={t('rollEditor.shotFrom')} describedBy={ids.dates}>
            {(props) => (
              <input
                {...props}
                type="text"
                value={from}
                placeholder="2026-07"
                onChange={(e) => setFrom(e.target.value)}
              />
            )}
          </Field>
          <Field label={t('rollEditor.shotTo')} describedBy={ids.dates}>
            {(props) => (
              <input
                {...props}
                type="text"
                value={to}
                placeholder="2026-08-02"
                onChange={(e) => setTo(e.target.value)}
              />
            )}
          </Field>
        </div>
        <p id={ids.dates} className={styles.hint}>
          {t('rollEditor.dateHint')}
        </p>
        <Field label={t('rollEditor.notes')}>
          {(props) => (
            <textarea
              {...props}
              rows={3}
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
            />
          )}
        </Field>
        {failure && <p role="alert">{failure}</p>}
        <div className={styles.buttons}>
          <Dialog.Close asChild>
            <button type="button" disabled={busy}>
              {t('rollEditor.cancel')}
            </button>
          </Dialog.Close>
          <button type="submit" disabled={busy}>
            {busy ? t('rollEditor.saving') : t('rollEditor.save')}
          </button>
        </div>
      </form>
    </>
  );
}

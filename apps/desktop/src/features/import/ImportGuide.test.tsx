import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useUiStore } from '../../app/store';
import { expectNoA11yViolations } from '../../test-support';
import { libraryDetails, makeRoll, resetFake, state } from '../library/fake-ipc';
import { resetLibraryStore, useLibraryStore } from '../library/library-store';
import { resetGuideStore, useGuideStore } from './guide-store';
import { ImportView } from './ImportView';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

type User = ReturnType<typeof userEvent.setup>;

/** A library with these rolls is open, as the Library view leaves it. */
function openLibrary(...rolls: ReturnType<typeof makeRoll>[]) {
  state.details = libraryDetails();
  state.rolls = rolls;
  useLibraryStore.setState({
    phase: 'ready',
    library: state.details,
    rolls,
    rollId: null,
  });
}

const stepBar = () => screen.getByRole('navigation', { name: 'Import steps' });
const current = () => within(stepBar()).getByRole('button', { current: 'step' });
const next = () => screen.getByRole('button', { name: /^(Next|Finish)$/ });
const back = () => screen.getByRole('button', { name: 'Back' });

/** The entries the settings were asked to keep, in order. */
const saved = () =>
  state.settingsPatches.flatMap((p) => (p.import_guide_entry ? [p.import_guide_entry] : []));

/** One whole triplet, as the file picker and the grouping answer. */
function oneTriplet() {
  const shot = (name: string, n: number) => ({
    path: `C:\\film\\${name}.NEF`,
    captured: `2026-10-06T19:43:0${n}.200`,
    channel: null,
  });
  state.pickedCamera = ['C:\\film\\DSC_0001.NEF'];
  state.triplets = [
    { files: [shot('DSC_0001', 1), shot('DSC_0002', 2), shot('DSC_0003', 3)], problem: null },
  ];
}

/** Chooses the triplet on the Files step. */
async function chooseTriplet(user: User) {
  oneTriplet();
  await user.click(screen.getByRole('button', { name: 'Choose camera files…' }));
  await screen.findByRole('list', { name: 'Triplets found' });
}

async function pick(user: User, roll: string, source: string) {
  await user.click(await screen.findByRole('radio', { name: roll }));
  await user.click(screen.getByRole('radio', { name: source }));
}

describe('ImportGuide', () => {
  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetGuideStore();
    useUiStore.setState({ view: 'import' });
  });

  it('shows the new roll form at once when the library has no roll, and saving it selects the roll', async () => {
    openLibrary();
    const user = userEvent.setup();
    render(<ImportView />);
    await user.type(await screen.findByRole('textbox', { name: 'Name' }), 'Beach');
    await user.click(screen.getByRole('button', { name: 'Create roll' }));

    expect(await screen.findByRole('radio', { name: 'Beach' })).toBeChecked();
    expect(screen.queryByRole('textbox', { name: 'Name' })).not.toBeInTheDocument();
    expect(state.rolls.map((r) => r.name)).toEqual(['Beach']);
    expect(useGuideStore.getState().lastRollId).toBe(state.rolls[0]?.id);
  });

  it('creates a new roll from the list of rolls, with the form validating its name', async () => {
    openLibrary(makeRoll({ name: 'Harbour' }));
    const user = userEvent.setup();
    render(<ImportView />);
    expect(screen.queryByRole('textbox', { name: 'Name' })).not.toBeInTheDocument();
    await user.click(await screen.findByRole('radio', { name: 'New roll' }));
    await user.click(screen.getByRole('button', { name: 'Create roll' }));
    expect(screen.getByText('Give the roll a name.')).toBeInTheDocument();
    expect(state.rolls).toHaveLength(1);

    await user.type(screen.getByRole('textbox', { name: 'Name' }), 'Beach');
    await user.click(screen.getByRole('button', { name: 'Create roll' }));
    expect(await screen.findByRole('radio', { name: 'Beach' })).toBeChecked();
    expect(state.rolls).toHaveLength(2);
  });

  it('keeps Next off until a roll and a source are chosen', async () => {
    openLibrary(makeRoll({ name: 'Harbour' }));
    const user = userEvent.setup();
    render(<ImportView />);
    await user.click(await screen.findByRole('radio', { name: 'Files' }));
    expect(next()).toBeDisabled();
    await user.click(screen.getByRole('radio', { name: 'Harbour' }));
    expect(next()).toBeEnabled();
  });

  it('keeps Next off with a roll but no source', async () => {
    openLibrary(makeRoll({ name: 'Harbour' }));
    const user = userEvent.setup();
    render(<ImportView />);
    await user.click(await screen.findByRole('radio', { name: 'Harbour' }));
    expect(next()).toBeDisabled();
    expect(back()).toBeDisabled();
  });

  it('shows the steps with the current one marked, the Roll step ticked and summed up, and jumps back', async () => {
    openLibrary(makeRoll({ name: 'Harbour', film_stock: 'Portra 400' }));
    const user = userEvent.setup();
    render(<ImportView />);
    await screen.findByRole('radio', { name: 'Harbour' });
    // Before anything is chosen there is only the Roll step, and it is not done.
    expect(within(stepBar()).getAllByRole('listitem')).toHaveLength(1);
    expect(current()).toHaveTextContent('Roll');
    expect(within(stepBar()).queryByText('Done')).not.toBeInTheDocument();

    await pick(user, 'Harbour', 'Scanner');
    const items = within(stepBar()).getAllByRole('listitem');
    expect(items.map((i) => i.querySelector('button')?.textContent)).toEqual([
      expect.stringContaining('Roll'),
      'Scanner',
      expect.stringContaining('Frames'),
      'Settings',
      'Scan',
    ]);
    expect(within(items[0] as HTMLElement).getByText('Done')).toBeInTheDocument();
    expect(stepBar()).toHaveTextContent('Harbour, Portra 400, Colour negative');
    expect(within(items.at(-1) as HTMLElement).queryByText('Done')).not.toBeInTheDocument();

    await user.click(next());
    expect(current()).toHaveTextContent('Scanner');
    expect(screen.getByRole('heading', { name: 'Find the scanner' })).toBeInTheDocument();
    await user.click(within(stepBar()).getByRole('button', { name: /^Roll/ }));
    expect(current()).toHaveTextContent('Roll');
    expect(screen.getByRole('heading', { name: 'Roll' })).toBeInTheDocument();
  });

  it('goes back and on with the buttons, and Finish replaces Next on the last step', async () => {
    openLibrary(makeRoll({ name: 'Harbour' }));
    const user = userEvent.setup();
    render(<ImportView />);
    await pick(user, 'Harbour', 'Files');
    expect(next()).toHaveTextContent('Next');
    await user.click(next());
    await chooseTriplet(user);
    await user.click(next());
    expect(next()).toHaveTextContent('Finish');
    expect(back()).toBeEnabled();
    await user.click(back());
    expect(current()).toHaveTextContent('Files');
    await user.click(back());
    expect(current()).toHaveTextContent('Roll');
    expect(back()).toBeDisabled();
  });

  it('goes back and on with Alt and the arrow keys', async () => {
    openLibrary(makeRoll({ name: 'Harbour' }));
    const user = userEvent.setup();
    render(<ImportView />);
    await pick(user, 'Harbour', 'Files');
    await user.keyboard('{Alt>}{ArrowLeft}{/Alt}');
    expect(current()).toHaveTextContent('Roll');
    await user.keyboard('{Alt>}{ArrowRight}{/Alt}');
    expect(current()).toHaveTextContent('Files');
    await user.keyboard('{Alt>}{ArrowLeft}{/Alt}');
    expect(current()).toHaveTextContent('Roll');
  });

  it('ignores Alt and the arrow keys while typing in a text field, and when the button is off', async () => {
    openLibrary(makeRoll({ name: 'Harbour' }));
    const user = userEvent.setup();
    render(<ImportView />);
    // No source yet: Next is off, so the key does nothing.
    await user.click(await screen.findByRole('radio', { name: 'Harbour' }));
    await user.keyboard('{Alt>}{ArrowRight}{/Alt}');
    expect(current()).toHaveTextContent('Roll');

    await user.click(screen.getByRole('radio', { name: 'Files' }));
    await user.click(screen.getByRole('radio', { name: 'New roll' }));
    await user.click(screen.getByRole('textbox', { name: 'Name' }));
    await user.keyboard('{Alt>}{ArrowRight}{/Alt}');
    expect(current()).toHaveTextContent('Roll');
    expect(screen.getByRole('heading', { name: 'Roll' })).toBeInTheDocument();
  });

  it('moves focus to the heading of each page', async () => {
    openLibrary(makeRoll({ name: 'Harbour' }));
    const user = userEvent.setup();
    render(<ImportView />);
    await pick(user, 'Harbour', 'Files');
    await user.click(next());
    expect(screen.getByRole('heading', { name: 'Choose the files' })).toHaveFocus();
    await user.click(back());
    expect(screen.getByRole('heading', { name: 'Roll' })).toHaveFocus();
  });

  it('saves the source and the step of the roll as they change', async () => {
    const roll = makeRoll({ name: 'Harbour' });
    openLibrary(roll);
    const user = userEvent.setup();
    render(<ImportView />);
    await pick(user, 'Harbour', 'Scanner');
    expect(saved()).toEqual([{ roll_id: roll.id, entry: { source: 'scanner', step: 'roll' } }]);
    await user.click(next());
    expect(saved().at(-1)).toEqual({
      roll_id: roll.id,
      entry: { source: 'scanner', step: 'scanner' },
    });
    await user.click(back());
    expect(saved().at(-1)).toEqual({
      roll_id: roll.id,
      entry: { source: 'scanner', step: 'roll' },
    });
    await user.click(screen.getByRole('radio', { name: 'Camera capture' }));
    expect(saved().at(-1)).toEqual({ roll_id: roll.id, entry: { source: 'camera', step: 'roll' } });
  });

  it('resumes the roll worked on last, at its source and step', async () => {
    const roll = makeRoll({ name: 'Harbour' });
    openLibrary(roll, makeRoll({ name: 'Beach' }));
    state.importGuide[roll.id] = { source: 'scanner', step: 'scan' };
    useGuideStore.getState().setLastRoll(roll.id);
    render(<ImportView />);
    expect(await screen.findByRole('heading', { name: 'Scan the roll' })).toBeInTheDocument();
    expect(current()).toHaveTextContent(/^Scan$/);
    expect(saved()).toEqual([]);
  });

  it('offers camera capture only while the guided capture flag is on, and does not resume at it', async () => {
    const roll = makeRoll({ name: 'Harbour' });
    openLibrary(roll);
    state.flags = { ...state.flags, guided_capture: false };
    state.importGuide[roll.id] = { source: 'camera', step: 'capture' };
    useGuideStore.getState().setLastRoll(roll.id);
    render(<ImportView />);

    expect(await screen.findByText(/Turn on Guided capture in Settings/)).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: 'Camera capture' })).toBeDisabled();
    expect(screen.getByRole('heading', { name: 'Roll' })).toBeInTheDocument();
    expect(next()).toBeDisabled();
    expect(screen.getByRole('radio', { name: 'Scanner' })).toBeEnabled();
  });

  it('lists the rolls with a saved place first, names the step to resume at, and resumes there', async () => {
    const old = makeRoll({ name: 'Harbour', created_at: '2026-01-01T10:00:00Z' });
    const newer = makeRoll({ name: 'Beach', created_at: '2026-06-01T10:00:00Z' });
    const newest = makeRoll({ name: 'Park', created_at: '2026-09-01T10:00:00Z' });
    openLibrary(newer, old, newest);
    state.importGuide[old.id] = { source: 'files', step: 'files' };
    const user = userEvent.setup();
    render(<ImportView />);
    await screen.findByRole('radio', { name: /Harbour/ });
    const radios = within(screen.getByRole('group', { name: 'Roll' })).getAllByRole('radio');
    expect(radios.map((r) => r.closest('label')?.textContent)).toEqual([
      'Harbour Resume at Files',
      'Park',
      'Beach',
      'New roll',
    ]);

    await user.click(screen.getByRole('radio', { name: /Harbour/ }));
    expect(screen.getByRole('radio', { name: 'Files' })).toBeChecked();
    await user.click(next());
    expect(current()).toHaveTextContent('Files');
  });

  it.each([
    ['Camera capture', 'Set up the camera and light', false],
    ['Scanner', 'Find the scanner', true],
    ['Files', 'Choose the files', false],
  ])('shows the first page of the %s source', async (source, heading, on) => {
    openLibrary(makeRoll({ name: 'Harbour' }), makeRoll({ name: 'Beach' }));
    const user = userEvent.setup();
    render(<ImportView />);
    await pick(user, 'Harbour', source);
    await user.click(next());
    expect(await screen.findByRole('heading', { name: heading })).toBeInTheDocument();
    expect(screen.queryByRole('combobox', { name: 'Import into roll' })).not.toBeInTheDocument();
    // Each first page has to be done before Next: the camera's light connected, a scanner found,
    // files chosen.
    await waitFor(() => {
      if (on) expect(screen.getByRole('button', { name: 'Next' })).toBeEnabled();
      else expect(screen.getByRole('button', { name: 'Next' })).toBeDisabled();
    });
  });

  it('ends in the Library with the roll selected', async () => {
    const roll = makeRoll({ name: 'Harbour' });
    openLibrary(makeRoll({ name: 'Beach' }), roll);
    const user = userEvent.setup();
    render(<ImportView />);
    await pick(user, 'Harbour', 'Files');
    await user.click(next());
    await chooseTriplet(user);
    await user.click(next());
    await user.click(screen.getByRole('button', { name: 'Finish' }));
    expect(useUiStore.getState().view).toBe('library');
    expect(useLibraryStore.getState().rollId).toBe(roll.id);
  });

  it('has no accessibility violations on the Roll page and on a step page', async () => {
    openLibrary(makeRoll({ name: 'Harbour' }));
    const user = userEvent.setup();
    const { container } = render(<ImportView />);
    await pick(user, 'Harbour', 'Files');
    await expectNoA11yViolations(container);
    await user.click(next());
    await screen.findByRole('heading', { name: 'Choose the files' });
    await expectNoA11yViolations(container);
  });
});

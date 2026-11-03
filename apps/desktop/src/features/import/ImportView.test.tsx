import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../test-support';
import type { TripletCandidate } from '../../shared/ipc';
import { ipc, libraryDetails, listeners, makeRoll, resetFake, state } from '../library/fake-ipc';
import { resetLibraryStore, useLibraryStore } from '../library/library-store';
import { ImportView } from './ImportView';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

const at = (n: number) => `2026-10-06T19:43:${String(n).padStart(2, '0')}.200`;
const shot = (name: string, n: number) => ({
  path: `C:\\film\\${name}.NEF`,
  captured: at(n),
  channel: null,
});
const whole = (a: string, b: string, c: string, n: number): TripletCandidate => ({
  files: [shot(a, n), shot(b, n + 1), shot(c, n + 2)],
  problem: null,
});

const picked = ['C:\\film\\DSC_0001.NEF'];

/** A library with a roll is open, as the Library view leaves it. */
function openLibrary(over = {}) {
  state.details = libraryDetails(over);
  const roll = makeRoll({ name: 'Harbour' });
  state.rolls = [roll];
  useLibraryStore.setState({
    phase: 'ready',
    library: state.details,
    rolls: [roll],
    rollId: roll.id,
  });
  return roll;
}

async function choose(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole('button', { name: 'Choose camera files…' }));
  await screen.findByRole('list', { name: 'Triplets found' });
}

describe('ImportView: camera files', () => {
  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    state.pickedCamera = picked;
  });

  it('asks for a library first when none is open', () => {
    render(<ImportView />);
    expect(screen.getByRole('status')).toHaveTextContent('Open or create a library');
    expect(screen.queryByRole('button', { name: 'Choose camera files…' })).not.toBeInTheDocument();
  });

  it('asks for a roll when the library has none', () => {
    state.details = libraryDetails();
    useLibraryStore.setState({ phase: 'ready', library: state.details, rolls: [] });
    render(<ImportView />);
    expect(screen.getByText(/has no roll yet/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Choose camera files…' })).not.toBeInTheDocument();
  });

  it('shows the grouped triplets with each file in its channel, and nothing is sent yet', async () => {
    openLibrary();
    state.triplets = [
      whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0),
      whole('DSC_0004', 'DSC_0005', 'DSC_0006', 10),
    ];
    const user = userEvent.setup();
    render(<ImportView />);
    await choose(user);

    expect(ipc.tripletGroup).toHaveBeenCalledWith(picked);
    expect(screen.getByText('2 triplets ready to import')).toBeInTheDocument();
    const first = screen.getByRole('list', { name: 'Files of triplet 1' });
    const rows = within(first).getAllByRole('listitem');
    expect(rows[0]).toHaveTextContent('red');
    expect(rows[0]).toHaveTextContent('DSC_0001.NEF');
    expect(rows[1]).toHaveTextContent('green');
    expect(rows[2]).toHaveTextContent('blue');
    expect(rows[0]).toHaveTextContent(/Oct 6, 2026/);
    expect(screen.getByRole('checkbox', { name: 'Import triplet 1' })).toBeChecked();
    expect(ipc.importFiles).not.toHaveBeenCalled();
  });

  it('puts the files in the other order with the move buttons, and sends them in that order', async () => {
    const roll = openLibrary();
    state.triplets = [whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0)];
    const user = userEvent.setup();
    render(<ImportView />);
    await choose(user);

    await user.click(screen.getByRole('button', { name: 'Move DSC_0001.NEF later' }));
    const rows = within(screen.getByRole('list', { name: 'Files of triplet 1' })).getAllByRole(
      'listitem',
    );
    expect(rows[0]).toHaveTextContent('red');
    expect(rows[0]).toHaveTextContent('DSC_0002.NEF');
    expect(rows[1]).toHaveTextContent('green');
    expect(rows[1]).toHaveTextContent('DSC_0001.NEF');
    expect(screen.getByRole('button', { name: 'Move DSC_0002.NEF earlier' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Move DSC_0003.NEF later' })).toBeDisabled();

    await user.click(screen.getByRole('button', { name: 'Move DSC_0003.NEF earlier' }));
    await user.click(screen.getByRole('button', { name: 'Import 1 triplet' }));
    expect(ipc.importFiles).toHaveBeenCalledWith(
      ['C:\\film\\DSC_0002.NEF', 'C:\\film\\DSC_0003.NEF', 'C:\\film\\DSC_0001.NEF'],
      roll.id,
      'RgbTriplet',
    );
  });

  it('shows a group with a problem with its message, and does not import it', async () => {
    const roll = openLibrary();
    state.triplets = [
      whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0),
      {
        files: [shot('DSC_0004', 10), shot('DSC_0005', 11)],
        problem: { kind: 'missing_shots', missing: 1 },
      },
      {
        files: [
          shot('DSC_0006', 20),
          shot('DSC_0007', 21),
          shot('DSC_0008', 22),
          shot('DSC_0009', 23),
        ],
        problem: { kind: 'extra_shots', extra: 1 },
      },
      {
        files: [{ path: 'C:\\film\\DSC_0010.NEF', captured: null, channel: null }],
        problem: { kind: 'no_capture_time' },
      },
      {
        files: [shot('DSC_0011', 40), shot('DSC_0012', 41), shot('DSC_0013', 42)],
        problem: { kind: 'channel_mismatch' },
      },
    ];
    const user = userEvent.setup();
    render(<ImportView />);
    await choose(user);

    expect(screen.getByText(/1 triplet ready to import/)).toHaveTextContent(
      '4 groups need attention',
    );
    const alerts = screen.getAllByRole('alert').map((a) => a.textContent);
    expect(alerts).toEqual([
      '1 shot is missing, so these files are not a whole triplet.',
      '1 shot too many: these files do not make whole triplets.',
      'These files have no capture time, so they could not be grouped.',
      'Two of these files were lit in the same colour, so they cannot be red, green and blue.',
    ]);
    // No confirm box or move buttons on them, and the files are numbered, not named by channel.
    // (The scanner section above has its own checkbox, for the infrared.)
    const found = within(screen.getByRole('list', { name: 'Triplets found' }));
    expect(found.getAllByRole('checkbox')).toHaveLength(1);
    expect(screen.queryByRole('button', { name: /Move DSC_0004/ })).not.toBeInTheDocument();
    expect(
      within(screen.getByRole('list', { name: 'Files of triplet 2' })).getAllByRole('listitem')[0],
    ).toHaveTextContent('Shot 1');
    expect(screen.getByText('no capture time')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Import 1 triplet' }));
    expect(ipc.importFiles).toHaveBeenCalledWith(
      ['C:\\film\\DSC_0001.NEF', 'C:\\film\\DSC_0002.NEF', 'C:\\film\\DSC_0003.NEF'],
      roll.id,
      'RgbTriplet',
    );
  });

  it('imports only the triplets that stay confirmed, and turns the button off when none does', async () => {
    openLibrary();
    state.triplets = [
      whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0),
      whole('DSC_0004', 'DSC_0005', 'DSC_0006', 10),
    ];
    const user = userEvent.setup();
    render(<ImportView />);
    await choose(user);

    await user.click(screen.getByRole('checkbox', { name: 'Import triplet 1' }));
    expect(screen.getByText('1 triplet ready to import')).toBeInTheDocument();
    await user.click(screen.getByRole('checkbox', { name: 'Import triplet 2' }));
    expect(screen.getByRole('button', { name: 'Import 0 triplets' })).toBeDisabled();
    await user.click(screen.getByRole('checkbox', { name: 'Import triplet 2' }));
    await user.click(screen.getByRole('button', { name: 'Import 1 triplet' }));
    expect(ipc.importFiles.mock.calls[0]?.[0]).toEqual([
      'C:\\film\\DSC_0004.NEF',
      'C:\\film\\DSC_0005.NEF',
      'C:\\film\\DSC_0006.NEF',
    ]);
  });

  it('imports into the roll that was chosen', async () => {
    const first = openLibrary();
    const second = makeRoll({ name: 'Beach' });
    state.rolls = [first, second];
    useLibraryStore.setState({ rolls: [first, second] });
    state.triplets = [whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0)];
    const user = userEvent.setup();
    render(<ImportView />);
    await user.selectOptions(screen.getByRole('combobox', { name: 'Import into roll' }), 'Beach');
    await choose(user);
    await user.click(screen.getByRole('button', { name: 'Import 1 triplet' }));
    expect(ipc.importFiles.mock.calls[0]?.[1]).toBe(second.id);
  });

  it('shows the progress with Cancel, then what came of the import: skipped files and warnings', async () => {
    openLibrary();
    state.triplets = [whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0)];
    ipc.importFiles.mockResolvedValueOnce('job-triplets');
    const user = userEvent.setup();
    render(<ImportView />);
    await choose(user);
    await user.click(screen.getByRole('button', { name: 'Import 1 triplet' }));

    expect(await screen.findByText(/The import started/)).toBeInTheDocument();
    expect(screen.queryByRole('list', { name: 'Triplets found' })).not.toBeInTheDocument();
    const bar = await screen.findByRole('progressbar', { name: 'Import into Harbour' });
    expect(bar).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(ipc.jobCancel).toHaveBeenCalledWith('job-triplets');

    act(() =>
      listeners.done.forEach((cb) =>
        cb({
          id: 'job-triplets',
          result: {
            kind: 'Imported',
            frames: ['frame-a', 'frame-b'],
            frame_ms: [5000, 4900],
            skipped: [{ path: 'C:\\film\\DSC_0009.NEF', reason: 'Unsupported', details: null }],
            warnings: [
              { frame_id: 'frame-b', kind: { kind: 'exposure_time_mismatch' } },
              { frame_id: 'frame-a', kind: { kind: 'clipped', channel: 'red', per_mille: 125 } },
            ],
          } as never,
        }),
      ),
    );
    await waitFor(() => expect(screen.getByText(/2 frames imported/)).toBeInTheDocument());
    expect(screen.getByText(/1 file skipped/)).toBeInTheDocument();
    expect(screen.getByText(/2 warnings/)).toBeInTheDocument();
    await user.click(screen.getByText('Warnings'));
    expect(
      screen.getByText('Frame 2: the three shots used different shutter times'),
    ).toBeInTheDocument();
    expect(
      screen.getByText('Frame 1: the red-lit shot is clipped in 12.5 % of its picture'),
    ).toBeInTheDocument();
    expect(screen.getByText(/DSC_0009.NEF: not a file the app can import/)).toBeInTheDocument();
  });

  it('says why the files could not be grouped, and why the import could not start', async () => {
    openLibrary();
    state.tripletsFail = 'The disk is not there.';
    const user = userEvent.setup();
    render(<ImportView />);
    await user.click(screen.getByRole('button', { name: 'Choose camera files…' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The files could not be grouped: The disk is not there.',
    );

    state.tripletsFail = null;
    state.triplets = [whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0)];
    ipc.importFiles.mockRejectedValueOnce(new Error('The library is read-only.'));
    await choose(user);
    await user.click(screen.getByRole('button', { name: 'Import 1 triplet' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The import could not be started: The library is read-only.',
    );
    // The triplets stay, so the user can try again.
    expect(screen.getByRole('checkbox', { name: 'Import triplet 1' })).toBeChecked();
  });

  it('does nothing when the picker is cancelled, and is off in a read-only library', async () => {
    openLibrary();
    state.pickedCamera = [];
    const user = userEvent.setup();
    render(<ImportView />);
    await user.click(screen.getByRole('button', { name: 'Choose camera files…' }));
    expect(ipc.tripletGroup).not.toHaveBeenCalled();
    expect(screen.queryByRole('list', { name: 'Triplets found' })).not.toBeInTheDocument();

    openLibrary({ read_only: true });
    render(<ImportView />);
    expect(screen.getAllByRole('button', { name: 'Choose camera files…' })[1]).toBeDisabled();
  });

  it('has no accessibility violations: empty, with triplets and groups with problems, and with a result', async () => {
    openLibrary();
    const { container } = render(<ImportView />);
    await expectNoA11yViolations(container);

    state.triplets = [
      whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0),
      {
        files: [shot('DSC_0004', 10), shot('DSC_0005', 11)],
        problem: { kind: 'missing_shots', missing: 1 },
      },
    ];
    const user = userEvent.setup();
    await choose(user);
    await expectNoA11yViolations(container);

    ipc.importFiles.mockResolvedValueOnce('job-a11y');
    await user.click(screen.getByRole('button', { name: 'Import 1 triplet' }));
    await screen.findByRole('progressbar', { name: 'Import into Harbour' });
    act(() =>
      listeners.done.forEach((cb) =>
        cb({
          id: 'job-a11y',
          result: {
            kind: 'Imported',
            frames: ['frame-a'],
            frame_ms: [5000],
            skipped: [],
            warnings: [{ frame_id: 'frame-a', kind: { kind: 'iso_mismatch' } }],
          } as never,
        }),
      ),
    );
    await screen.findByText(/1 warning/);
    await expectNoA11yViolations(container);
  });
});

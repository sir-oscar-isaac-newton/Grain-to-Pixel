import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ipc, resetFake } from '../../library/fake-ipc';
import { LiveView, RESUME_SETTLE_MS } from './LiveView';

vi.mock('../../../shared/ipc', async () => (await import('../../library/fake-ipc')).ipc);

type User = ReturnType<typeof userEvent.setup>;

interface Props {
  blocked: boolean;
  lightLost?: boolean;
  resumeAfterBlock?: boolean;
}

function View({ blocked, lightLost = false, resumeAfterBlock = true }: Props) {
  return (
    <LiveView
      calibrationArea={null}
      onCalibrationAreaChange={vi.fn()}
      levels={[40, 20, 4]}
      lightLost={lightLost}
      blocked={blocked}
      resumeAfterBlock={resumeAfterBlock}
      stopRef={{ current: null }}
    />
  );
}

const startButton = () => screen.getByRole('button', { name: 'Start live view' });
const stopButton = () => screen.queryByRole('button', { name: 'Stop live view' });
const wait = (ms: number) => act(() => new Promise<void>((resolve) => setTimeout(resolve, ms)));

/** Renders the view running, optionally with a light colour on; `again` sets new props. */
async function running(user: User, props: Partial<Props> = {}, colour: string | null = null) {
  const view = render(<View blocked={false} {...props} />);
  await user.click(startButton());
  await screen.findByRole('button', { name: 'Stop live view' });
  if (colour) await user.click(screen.getByRole('radio', { name: colour }));
  const again = (next: Props) => view.rerender(<View {...props} {...next} />);
  return { ...view, again };
}

beforeEach(() => {
  resetFake();
  vi.clearAllMocks();
  URL.createObjectURL = vi.fn(() => 'blob:frame');
  URL.revokeObjectURL = vi.fn();
});
afterEach(cleanup);

describe('live view after a block', () => {
  it('starts again with its light colour once the block has ended', async () => {
    const user = userEvent.setup();
    const { again } = await running(user, {}, 'Green');
    await waitFor(() => expect(ipc.lightSet).toHaveBeenLastCalledWith([0, 20, 0]));
    again({ blocked: true });
    await waitFor(() => expect(stopButton()).toBeNull());
    expect(screen.getByRole('radio', { name: 'Off' })).toBeChecked();
    ipc.cameraPreviewFrame.mockClear();
    ipc.lightSet.mockClear();
    again({ blocked: false });
    // Not at once: the block has to stay away for a moment.
    expect(stopButton()).toBeNull();
    expect(await screen.findByRole('button', { name: 'Stop live view' })).toBeInTheDocument();
    await waitFor(() => expect(ipc.cameraPreviewFrame).toHaveBeenCalled());
    expect(screen.getByRole('radio', { name: 'Green' })).toBeChecked();
    await waitFor(() => expect(ipc.lightSet).toHaveBeenLastCalledWith([0, 20, 0]));
  });

  it('starts again without a light colour when none was on', async () => {
    const user = userEvent.setup();
    const { again } = await running(user);
    again({ blocked: true });
    await waitFor(() => expect(stopButton()).toBeNull());
    again({ blocked: false });
    await screen.findByRole('button', { name: 'Stop live view' });
    expect(screen.getByRole('radio', { name: 'Off' })).toBeChecked();
    expect(ipc.lightSet).not.toHaveBeenCalled();
  });

  it('does not start in the middle of a block that flickers', async () => {
    const user = userEvent.setup();
    const { again } = await running(user, {}, 'Red');
    again({ blocked: true });
    await waitFor(() => expect(stopButton()).toBeNull());
    ipc.cameraPreviewFrame.mockClear();
    // The block drops for a moment, comes back, and drops for good only later.
    again({ blocked: false });
    await wait(RESUME_SETTLE_MS / 2);
    again({ blocked: true });
    await wait(RESUME_SETTLE_MS * 1.5);
    expect(ipc.cameraPreviewFrame).not.toHaveBeenCalled();
    expect(stopButton()).toBeNull();
    again({ blocked: false });
    await wait(RESUME_SETTLE_MS / 2);
    expect(ipc.cameraPreviewFrame).not.toHaveBeenCalled();
    await screen.findByRole('button', { name: 'Stop live view' });
    expect(screen.getByRole('radio', { name: 'Red' })).toBeChecked();
  });

  it('starts again once only, however often the block came and went', async () => {
    const user = userEvent.setup();
    const { again } = await running(user);
    again({ blocked: true });
    again({ blocked: false });
    again({ blocked: true });
    again({ blocked: false });
    await screen.findByRole('button', { name: 'Stop live view' });
    await wait(RESUME_SETTLE_MS * 1.5);
    // The first start and the one after the block.
    expect(ipc.cameraPreviewZoomSteps).toHaveBeenCalledTimes(2);
  });

  it('does not start again when it was not running', async () => {
    const { rerender } = render(<View blocked={false} />);
    rerender(<View blocked />);
    rerender(<View blocked={false} />);
    await wait(RESUME_SETTLE_MS * 1.5);
    expect(ipc.cameraPreviewFrame).not.toHaveBeenCalled();
    expect(startButton()).toBeEnabled();
  });

  it('does not start again after the user stopped it', async () => {
    const user = userEvent.setup();
    const { again } = await running(user);
    await user.click(screen.getByRole('button', { name: 'Stop live view' }));
    ipc.cameraPreviewFrame.mockClear();
    again({ blocked: true });
    again({ blocked: false });
    await wait(RESUME_SETTLE_MS * 1.5);
    expect(ipc.cameraPreviewFrame).not.toHaveBeenCalled();
    expect(stopButton()).toBeNull();
  });

  it('does not start again when the light was lost meanwhile', async () => {
    const user = userEvent.setup();
    const { again } = await running(user, {}, 'Blue');
    again({ blocked: true });
    again({ blocked: true, lightLost: true });
    again({ blocked: false, lightLost: true });
    await wait(RESUME_SETTLE_MS * 1.5);
    expect(stopButton()).toBeNull();
    // Nor when the light is back: the loss was the end of it.
    again({ blocked: false, lightLost: false });
    await wait(RESUME_SETTLE_MS * 1.5);
    expect(stopButton()).toBeNull();
    expect(screen.getByRole('radio', { name: 'Off' })).toBeChecked();
  });

  it('does not start again when the light was already lost as the block began', async () => {
    const user = userEvent.setup();
    const { again } = await running(user);
    again({ blocked: true, lightLost: true });
    again({ blocked: false, lightLost: true });
    await wait(RESUME_SETTLE_MS * 1.5);
    expect(stopButton()).toBeNull();
  });

  it('does not start again after an error was shown', async () => {
    const user = userEvent.setup();
    const { again } = await running(user);
    // The zoom is refused: a message shows while the view keeps running.
    ipc.cameraPreviewSetZoom.mockRejectedValueOnce(new Error('Refused.'));
    await user.click(await screen.findByRole('radio', { name: '25 %' }));
    await screen.findByRole('alert');
    again({ blocked: true });
    again({ blocked: false });
    await wait(RESUME_SETTLE_MS * 1.5);
    expect(stopButton()).toBeNull();
  });

  it('does not start again when the user started it meanwhile, and keeps one loop', async () => {
    const user = userEvent.setup();
    const { again } = await running(user);
    again({ blocked: true });
    again({ blocked: false });
    await user.click(startButton());
    await wait(RESUME_SETTLE_MS * 1.5);
    expect(ipc.cameraPreviewZoomSteps).toHaveBeenCalledTimes(2);
    expect(stopButton()).not.toBeNull();
  });

  it('does not start again after it was closed', async () => {
    const user = userEvent.setup();
    const { again, unmount } = await running(user);
    again({ blocked: true });
    again({ blocked: false });
    ipc.cameraPreviewFrame.mockClear();
    unmount();
    await wait(RESUME_SETTLE_MS * 1.5);
    expect(ipc.cameraPreviewFrame).not.toHaveBeenCalled();
  });

  it('does not start again unless asked to', async () => {
    const user = userEvent.setup();
    const { again } = await running(user, { resumeAfterBlock: false });
    again({ blocked: true });
    again({ blocked: false });
    await wait(RESUME_SETTLE_MS * 1.5);
    expect(stopButton()).toBeNull();
  });
});

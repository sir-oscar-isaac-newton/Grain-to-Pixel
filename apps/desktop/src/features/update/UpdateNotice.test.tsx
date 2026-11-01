import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ipc, resetFake, state } from '../library/fake-ipc';
import { UpdateNotice } from './UpdateNotice';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

const available = {
  status: 'available',
  version: '0.2.0',
  current_version: '0.1.0',
  notes: null,
} as const;

describe('UpdateNotice', () => {
  beforeEach(() => resetFake());

  it('asks once, as the launch check and not the button', async () => {
    render(<UpdateNotice />);
    await waitFor(() => expect(ipc.updateCheck).toHaveBeenCalledTimes(1));
    expect(ipc.updateCheck).toHaveBeenCalledWith(false);
  });

  it.each([
    ['is up to date', { status: 'up_to_date' }],
    ['is not due yet', { status: 'not_due' }],
    ['is switched off', { status: 'disabled' }],
  ] as const)('shows nothing when the app %s', async (_name, outcome) => {
    state.updateOutcome = outcome;
    render(<UpdateNotice />);
    await waitFor(() => expect(ipc.updateCheck).toHaveBeenCalled());
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it('offers the update that was found, and Later puts it away', async () => {
    state.updateOutcome = available;
    const user = userEvent.setup();
    render(<UpdateNotice />);
    expect(await screen.findByRole('status')).toHaveTextContent('Version 0.2.0 is available');
    await user.click(screen.getByRole('button', { name: 'Later' }));
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    expect(ipc.updateInstall).not.toHaveBeenCalled();
  });

  it('says nothing when the check fails', async () => {
    state.updateCheckFails = 'Could not reach the update server.';
    render(<UpdateNotice />);
    await waitFor(() => expect(ipc.updateCheck).toHaveBeenCalled());
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });
});

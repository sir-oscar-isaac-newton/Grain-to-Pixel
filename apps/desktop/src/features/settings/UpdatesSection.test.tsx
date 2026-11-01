import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../test-support';
import { ipc, resetFake, state } from '../library/fake-ipc';
import { UpdatesSection } from './UpdatesSection';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

const button = () => screen.getByRole('button', { name: /Check for updates now|Checking/ });

describe('UpdatesSection', () => {
  beforeEach(() => resetFake());

  it('checks on the button, as a manual check', async () => {
    const user = userEvent.setup();
    render(<UpdatesSection enabled />);
    expect(ipc.updateCheck).not.toHaveBeenCalled();
    await user.click(button());
    expect(ipc.updateCheck).toHaveBeenCalledWith(true);
    expect(await screen.findByText('You have the newest version.')).toBeInTheDocument();
  });

  it('offers a newer version, which is installed only on Install', async () => {
    state.updateOutcome = {
      status: 'available',
      version: '0.2.0',
      current_version: '0.1.0',
      notes: 'New things',
    };
    const user = userEvent.setup();
    render(<UpdatesSection enabled />);
    await user.click(button());
    expect(await screen.findByText(/Version 0.2.0 is available/)).toBeInTheDocument();
    expect(ipc.updateInstall).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Later' }));
    expect(screen.queryByText(/Version 0.2.0 is available/)).not.toBeInTheDocument();
  });

  it('says why a check failed, unlike the quiet check at launch', async () => {
    state.updateCheckFails = 'Could not reach the update server.';
    const user = userEvent.setup();
    render(<UpdatesSection enabled />);
    await user.click(button());
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not check for updates: Could not reach the update server.',
    );
    expect(button()).toBeEnabled();
  });

  it('turns the button off, and says why, while the update check is switched off', async () => {
    render(<UpdatesSection enabled={false} />);
    expect(button()).toBeDisabled();
    expect(screen.getByText(/switched off/)).toBeInTheDocument();
    expect(ipc.updateCheck).not.toHaveBeenCalled();
  });

  it('has no accessibility violations', async () => {
    const { container } = render(<UpdatesSection enabled />);
    await expectNoA11yViolations(container);
  });
});

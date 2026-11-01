import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../test-support';
import { ipc, resetFake, state } from '../library/fake-ipc';
import { UpdateOffer, type UpdateOfferInfo } from './UpdateOffer';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

const offer: UpdateOfferInfo = {
  version: '0.2.0',
  current_version: '0.1.0',
  notes: 'Dust removal is faster.',
};

function setup(over: Partial<UpdateOfferInfo> = {}) {
  const onLater = vi.fn();
  render(<UpdateOffer offer={{ ...offer, ...over }} onLater={onLater} />);
  return { onLater, user: userEvent.setup() };
}

describe('UpdateOffer', () => {
  beforeEach(() => resetFake());

  it('says which version is available, what is new, and installs nothing by itself', () => {
    setup();
    expect(screen.getByRole('status')).toHaveTextContent('Version 0.2.0 is available');
    expect(screen.getByRole('status')).toHaveTextContent('You have 0.1.0');
    expect(screen.getByText('Dust removal is faster.')).toBeInTheDocument();
    expect(ipc.updateInstall).not.toHaveBeenCalled();
  });

  it('leaves out the notes when there are none', () => {
    setup({ notes: null });
    expect(screen.queryByText('What is new')).not.toBeInTheDocument();
  });

  it('shows the notes as plain text, never as markup', () => {
    setup({ notes: '<img src=x onerror=alert(1)> **bold**' });
    expect(screen.getByText(/<img src=x/)).toBeInTheDocument();
    expect(document.querySelector('img')).toBeNull();
  });

  it('asks the app to install only when Install is pressed, then offers the restart', async () => {
    const { user } = setup();
    await user.click(screen.getByRole('button', { name: 'Install' }));
    expect(ipc.updateInstall).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(/The update is installed/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Install' })).not.toBeInTheDocument();

    expect(ipc.updateRestart).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Restart now' }));
    expect(ipc.updateRestart).toHaveBeenCalledTimes(1);
  });

  it('turns both buttons off while the update downloads', async () => {
    let finish: () => void = () => undefined;
    ipc.updateInstall.mockImplementationOnce(
      () => new Promise<void>((resolve) => (finish = resolve)),
    );
    const { user } = setup();
    await user.click(screen.getByRole('button', { name: 'Install' }));
    expect(screen.getByText(/Downloading the update/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Install' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Later' })).toBeDisabled();
    finish();
    await screen.findByText(/The update is installed/);
  });

  it('says why an update was not installed and lets the user try again', async () => {
    state.updateInstallFails = 'The update could not be verified, so it was not installed.';
    const { user } = setup();
    await user.click(screen.getByRole('button', { name: 'Install' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('could not be verified');
    expect(screen.queryByText(/The update is installed/)).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Install' })).toBeEnabled();

    state.updateInstallFails = null;
    await user.click(screen.getByRole('button', { name: 'Install' }));
    await waitFor(() => expect(screen.getByText(/The update is installed/)).toBeInTheDocument());
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('puts the offer away on Later without installing', async () => {
    const { user, onLater } = setup();
    await user.click(screen.getByRole('button', { name: 'Later' }));
    expect(onLater).toHaveBeenCalledTimes(1);
    expect(ipc.updateInstall).not.toHaveBeenCalled();
  });

  it('has no accessibility violations', async () => {
    const { user } = setup();
    await expectNoA11yViolations(document.body);
    await user.click(screen.getByRole('button', { name: 'Install' }));
    await screen.findByText(/The update is installed/);
    await expectNoA11yViolations(document.body);
  });
});

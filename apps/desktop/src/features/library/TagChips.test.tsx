import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { TagChips } from '@gtp/ui';
import { expectNoA11yViolations } from '../../test-support';

const tags = [
  { id: 'a', name: 'Japan' },
  { id: 'b', name: 'street' },
];

describe('TagChips', () => {
  it('lists the tags in a labelled group, or the empty text', () => {
    const { rerender } = render(<TagChips label="Tags" tags={tags} />);
    expect(screen.getByRole('group', { name: 'Tags' })).toBeInTheDocument();
    expect(screen.getAllByRole('listitem').map((li) => li.textContent)).toEqual([
      'Japan',
      'street',
    ]);
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
    expect(screen.queryByRole('combobox')).not.toBeInTheDocument();

    rerender(<TagChips label="Tags" tags={[]} emptyText="Nothing here" />);
    expect(screen.getByText('Nothing here')).toBeInTheDocument();
    expect(screen.queryByRole('list')).not.toBeInTheDocument();
  });

  it('removes a tag with its named button', async () => {
    const user = userEvent.setup();
    const onRemove = vi.fn();
    render(
      <TagChips label="Tags" tags={tags} removeLabel={(n) => `Remove ${n}`} onRemove={onRemove} />,
    );
    await user.click(screen.getByRole('button', { name: 'Remove street' }));
    expect(onRemove).toHaveBeenCalledWith(tags[1]);
  });

  it('adds a trimmed name with Enter or the button, never an empty one, and then clears the box', async () => {
    const user = userEvent.setup();
    const onAdd = vi.fn();
    render(
      <TagChips
        label="Tags"
        tags={[]}
        onAdd={onAdd}
        addLabel="New tag"
        addButtonLabel="Add"
        suggestions={['Japan', 'street']}
      />,
    );
    const box = screen.getByRole('combobox', { name: 'New tag' });
    const add = screen.getByRole('button', { name: 'Add' });
    expect(add).toBeDisabled();
    await user.type(box, '   {Enter}');
    expect(onAdd).not.toHaveBeenCalled();

    await user.type(box, '  Night market {Enter}');
    expect(onAdd).toHaveBeenLastCalledWith('Night market');
    expect(box).toHaveValue('');

    await user.type(box, 'Rain');
    await user.click(add);
    expect(onAdd).toHaveBeenLastCalledWith('Rain');

    const list = document.getElementById(box.getAttribute('list') ?? '');
    expect(
      [...(list?.querySelectorAll('option') ?? [])].map((o) => o.getAttribute('value')),
    ).toEqual(['Japan', 'street']);
  });

  it('does not submit an enclosing form when Enter adds a tag', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn((e: { preventDefault: () => void }) => e.preventDefault());
    render(
      <form onSubmit={onSubmit}>
        <TagChips
          label="Tags"
          tags={[]}
          onAdd={() => undefined}
          addLabel="New tag"
          addButtonLabel="Add"
        />
      </form>,
    );
    await user.type(screen.getByRole('combobox', { name: 'New tag' }), 'x{Enter}');
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('disables everything when told to, and has no accessibility violations', async () => {
    const { container } = render(
      <TagChips
        label="Tags"
        tags={tags}
        removeLabel={(n) => `Remove ${n}`}
        onRemove={() => undefined}
        onAdd={() => undefined}
        addLabel="New tag"
        addButtonLabel="Add"
        disabled
      />,
    );
    expect(screen.getByRole('combobox', { name: 'New tag' })).toBeDisabled();
    screen.getAllByRole('button').forEach((b) => expect(b).toBeDisabled());
    await expectNoA11yViolations(container);
  });
});

import { describe, expect, it } from 'vitest';
import {
  ACTION_IDS,
  COLOUR_ACTIONS,
  DEFAULT_KEYS,
  actionForKey,
  bindingProblem,
  effectiveKeys,
  isValidKey,
  keyLabel,
  normalizeKey,
  overridesOf,
} from './keymap';

describe('the default keys', () => {
  it('map the keys the spec names to their actions', () => {
    const map = effectiveKeys(undefined);
    expect(actionForKey(map, '\\')).toBe('develop.toggleBefore');
    expect(actionForKey(map, '[')).toBe('develop.previousFrame');
    expect(actionForKey(map, ']')).toBe('develop.nextFrame');
    expect(actionForKey(map, 'Escape')).toBe('develop.backToLibrary');
    expect(actionForKey(map, '0')).toBe('develop.fit');
    expect(actionForKey(map, '1')).toBe('develop.actualSize');
    expect(actionForKey(map, '+')).toBe('develop.zoomIn');
    expect(actionForKey(map, '-')).toBe('develop.zoomOut');
  });

  it('put red, cyan, green, magenta, blue and yellow on Q, A, W, S, E and D', () => {
    expect(
      [
        'colour.red',
        'colour.cyan',
        'colour.green',
        'colour.magenta',
        'colour.blue',
        'colour.yellow',
      ].map((id) => DEFAULT_KEYS[id as keyof typeof DEFAULT_KEYS]),
    ).toEqual(['q', 'a', 'w', 's', 'e', 'd']);
  });

  it('move the sliders the way the spec says: red, green and blue are the plus side', () => {
    expect(COLOUR_ACTIONS['colour.red']).toEqual({ axis: 'red_cyan', sign: 1 });
    expect(COLOUR_ACTIONS['colour.cyan']).toEqual({ axis: 'red_cyan', sign: -1 });
    expect(COLOUR_ACTIONS['colour.green']).toEqual({ axis: 'green_magenta', sign: 1 });
    expect(COLOUR_ACTIONS['colour.magenta']).toEqual({ axis: 'green_magenta', sign: -1 });
    expect(COLOUR_ACTIONS['colour.blue']).toEqual({ axis: 'blue_yellow', sign: 1 });
    expect(COLOUR_ACTIONS['colour.yellow']).toEqual({ axis: 'blue_yellow', sign: -1 });
  });

  it('give every action its own key', () => {
    const keys = ACTION_IDS.map((id) => normalizeKey(DEFAULT_KEYS[id]));
    expect(new Set(keys).size).toBe(ACTION_IDS.length);
  });
});

describe('actionForKey', () => {
  const map = effectiveKeys(undefined);

  it('ignores case, so Shift never changes which action a key means', () => {
    expect(actionForKey(map, 'q')).toBe('colour.red');
    expect(actionForKey(map, 'Q')).toBe('colour.red');
    expect(actionForKey(map, 'D')).toBe('colour.yellow');
  });

  it('accepts = for + so Shift is not needed, and ignores other keys', () => {
    expect(actionForKey(map, '=')).toBe('develop.zoomIn');
    expect(actionForKey(map, 'z')).toBeNull();
    expect(actionForKey(map, 'Enter')).toBeNull();
    expect(actionForKey(map, 'ArrowLeft')).toBeNull();
  });

  it('lets = keep a binding of its own', () => {
    const own = effectiveKeys({ 'develop.fit': '=' });
    expect(actionForKey(own, '=')).toBe('develop.fit');
    expect(actionForKey(own, '+')).toBe('develop.zoomIn');
  });
});

describe('effectiveKeys', () => {
  it('is the defaults with none changed', () => {
    expect(effectiveKeys(undefined)).toEqual(DEFAULT_KEYS);
    expect(effectiveKeys({})).toEqual(DEFAULT_KEYS);
  });

  it('uses the keys the user changed', () => {
    const map = effectiveKeys({ 'colour.red': 'r', 'develop.fit': 'f' });
    expect(map['colour.red']).toBe('r');
    expect(map['develop.fit']).toBe('f');
    expect(map['colour.cyan']).toBe('a');
    expect(actionForKey(map, 'q')).toBeNull();
    expect(actionForKey(map, 'R')).toBe('colour.red');
  });

  it('lets two actions swap keys, since no key ends up on two', () => {
    const map = effectiveKeys({ 'colour.red': 'a', 'colour.cyan': 'q' });
    expect([map['colour.red'], map['colour.cyan']]).toEqual(['a', 'q']);
  });

  it('ignores choices that make no sense, so a file edited by hand cannot take the keys away', () => {
    const map = effectiveKeys({
      'colour.red': 'ab',
      'colour.cyan': ' ',
      'colour.green': '',
      'colour.magenta': '\n',
      'no.such.action': 'x',
      'develop.fit': 'f',
    });
    expect(map).toEqual({ ...DEFAULT_KEYS, 'develop.fit': 'f' });
  });

  it('ignores a choice that takes the key of another action', () => {
    // Red onto cyan's A: red keeps Q.
    expect(effectiveKeys({ 'colour.red': 'a' })['colour.red']).toBe('q');
    // The same, with the case different.
    expect(effectiveKeys({ 'colour.red': 'A' })['colour.red']).toBe('q');
    // Two choices for one key: neither is used.
    const both = effectiveKeys({ 'colour.red': 'x', 'colour.blue': 'X' });
    expect([both['colour.red'], both['colour.blue']]).toEqual(['q', 'e']);
    // The keys the clash left alone are kept.
    const some = effectiveKeys({ 'colour.red': 'a', 'develop.fit': 'f' });
    expect(some['develop.fit']).toBe('f');
  });

  it('keeps a default that equals the choice', () => {
    expect(effectiveKeys({ 'colour.red': 'q' })).toEqual(DEFAULT_KEYS);
  });
});

describe('overridesOf', () => {
  it('lists only the keys that differ from the defaults', () => {
    expect(overridesOf(DEFAULT_KEYS)).toEqual({});
    expect(overridesOf({ ...DEFAULT_KEYS, 'colour.red': 'r', 'develop.fit': 'f' })).toEqual({
      'colour.red': 'r',
      'develop.fit': 'f',
    });
  });
});

describe('bindingProblem', () => {
  const map = effectiveKeys(undefined);

  it('finds nothing wrong with a free key, or with the key the action has already', () => {
    expect(bindingProblem(map, 'colour.red', 'r')).toBeNull();
    expect(bindingProblem(map, 'colour.red', 'q')).toBeNull();
    expect(bindingProblem(map, 'colour.red', 'Q')).toBeNull();
    expect(bindingProblem(map, 'develop.fit', ',')).toBeNull();
  });

  it('names the action that has the key, whatever the case', () => {
    expect(bindingProblem(map, 'colour.red', 'a')).toEqual({ kind: 'taken', by: 'colour.cyan' });
    expect(bindingProblem(map, 'colour.red', 'A')).toEqual({ kind: 'taken', by: 'colour.cyan' });
    expect(bindingProblem(map, 'develop.fit', ']')).toEqual({
      kind: 'taken',
      by: 'develop.nextFrame',
    });
  });

  it('refuses keys that cannot be a shortcut, and Escape, which cancels the picking', () => {
    for (const key of [' ', 'Tab', 'Enter', 'ArrowUp', 'F5', 'ab', '', 'Escape']) {
      expect(bindingProblem(map, 'colour.red', key), JSON.stringify(key)).toEqual({
        kind: 'invalid',
      });
    }
  });
});

describe('keys as text', () => {
  it('knows which keys are valid', () => {
    for (const key of ['q', 'Q', '1', '[', '\\', '+', 'é', 'Escape'])
      expect(isValidKey(key)).toBe(true);
    for (const key of ['', ' ', 'ab', 'Tab', '\n', '\u0007']) expect(isValidKey(key)).toBe(false);
  });

  it('compares letters without case and leaves named keys alone', () => {
    expect(normalizeKey('Q')).toBe('q');
    expect(normalizeKey('É')).toBe('é');
    expect(normalizeKey('Escape')).toBe('Escape');
    expect(normalizeKey('[')).toBe('[');
  });

  it('shows letters in capitals and Escape as Esc', () => {
    expect(keyLabel('q')).toBe('Q');
    expect(keyLabel('Escape')).toBe('Esc');
    expect(keyLabel('[')).toBe('[');
  });
});

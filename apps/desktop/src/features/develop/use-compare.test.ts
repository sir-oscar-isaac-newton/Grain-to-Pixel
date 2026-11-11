import { describe, expect, it } from 'vitest';
import { edgeFor, zoomedEdge } from './use-compare';

describe('reference render size', () => {
  it('asks for the pane long edge in steps of 128, between 256 and 2048', () => {
    expect(edgeFor(0, 600)).toBeNull();
    expect(edgeFor(900, 600)).toBe(1024);
    expect(edgeFor(1024, 10)).toBe(1024);
    expect(edgeFor(1025, 10)).toBe(1152);
    expect(edgeFor(100, 50)).toBe(256);
    expect(edgeFor(5000, 3000)).toBe(2048);
  });

  it('asks for the long edge the zoom needs, in the same steps and limits', () => {
    // At zoom 1 the long edge is the proxy's 2048 px on screen.
    expect(zoomedEdge(1)).toBe(2048);
    expect(zoomedEdge(0.5)).toBe(1024);
    expect(zoomedEdge(0.5001)).toBe(1152);
    expect(zoomedEdge(0.6)).toBe(1280);
    expect(zoomedEdge(0.05)).toBe(256);
    expect(zoomedEdge(4)).toBe(2048);
  });

  it('does not step up for a zoom that is a multiple of a step up to rounding', () => {
    expect(zoomedEdge(0.5 + 1e-12)).toBe(1024);
  });
});

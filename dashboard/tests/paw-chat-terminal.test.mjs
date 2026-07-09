import test from 'node:test';
import assert from 'node:assert/strict';

import { fetchTerminalSessionSnapshot } from '../src/lib/stores/paw-chat-terminal.ts';

test('fetchTerminalSessionSnapshot waits for a completed Session result projection', async () => {
  const snapshots = [
    { Status: 'Completed', result: '', error_message: '', _events: [] },
    { Status: 'Completed', result: 'Hey — I’m here.', error_message: '', _events: [] },
  ];
  let calls = 0;

  const snapshot = await fetchTerminalSessionSnapshot(
    'ss-racy',
    'Completed',
    async () => snapshots[Math.min(calls++, snapshots.length - 1)],
    async () => {},
    [0, 0, 0]
  );

  assert.equal(calls, 2);
  assert.equal(snapshot.result, 'Hey — I’m here.');
  assert.equal(snapshot.errorMessage, '');
});

test('fetchTerminalSessionSnapshot stops waiting when a terminal error is available', async () => {
  let calls = 0;

  const snapshot = await fetchTerminalSessionSnapshot(
    'ss-failed',
    'Failed',
    async () => {
      calls += 1;
      return { Status: 'Failed', result: '', error_message: 'boom', _events: [] };
    },
    async () => {
      throw new Error('should not wait for failed sessions with an error');
    },
    [0, 0, 0]
  );

  assert.equal(calls, 1);
  assert.equal(snapshot.result, '');
  assert.equal(snapshot.errorMessage, 'boom');
});

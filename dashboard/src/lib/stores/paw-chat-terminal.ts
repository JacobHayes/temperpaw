export interface TerminalSessionSnapshot {
  result: string;
  errorMessage: string;
  rawEvents: unknown[];
}

export type TerminalSessionFetcher = (sessionId: string) => Promise<Record<string, unknown>>;
export type SleepFn = (milliseconds: number) => Promise<void>;

export const DEFAULT_TERMINAL_PROJECTION_WAIT_DELAYS_MS = [100, 250, 500, 1000];

function fieldString(entity: Record<string, unknown>, keys: string[]): string {
  for (const key of keys) {
    const value = entity[key];
    if (value === null || value === undefined) continue;
    const text = String(value);
    if (text.length > 0) return text;
  }
  return '';
}

function shouldAwaitTerminalProjection(
  terminalStatus: string,
  result: string,
  errorMessage: string,
  hasRemainingDelay: boolean
): boolean {
  if (!hasRemainingDelay) return false;
  if (result || errorMessage) return false;
  return terminalStatus === 'Completed' || terminalStatus === 'Failed' || terminalStatus === 'Cancelled';
}

function defaultSleep(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

/**
 * Terminal SSE events can arrive before the OData projection exposes the final
 * Session.result/error_message. Wait briefly for the read model to catch up so
 * the UI does not freeze an empty completed assistant bubble during that
 * projection race.
 */
export async function fetchTerminalSessionSnapshot(
  sessionId: string,
  terminalStatus: string,
  fetchEntity: TerminalSessionFetcher,
  sleep: SleepFn = defaultSleep,
  projectionWaitDelaysMs: number[] = DEFAULT_TERMINAL_PROJECTION_WAIT_DELAYS_MS
): Promise<TerminalSessionSnapshot> {
  let snapshot: TerminalSessionSnapshot = { result: '', errorMessage: '', rawEvents: [] };

  for (let attempt = 0; attempt <= projectionWaitDelaysMs.length; attempt += 1) {
    const entity = await fetchEntity(sessionId);
    const rawEvents = Array.isArray(entity._events) ? entity._events : [];
    snapshot = {
      result: fieldString(entity, ['result', 'Result']),
      errorMessage: fieldString(entity, ['error_message', 'ErrorMessage']),
      rawEvents,
    };

    if (!shouldAwaitTerminalProjection(
      terminalStatus,
      snapshot.result,
      snapshot.errorMessage,
      attempt < projectionWaitDelaysMs.length
    )) {
      return snapshot;
    }

    await sleep(projectionWaitDelaysMs[attempt]);
  }

  return snapshot;
}

export function missingCompletedResultMessage(sessionId: string): string {
  return `Session ${sessionId} completed, but no response text was visible in the read model yet. Refresh the page to re-read the completed Session.`;
}

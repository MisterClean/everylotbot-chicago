export interface Logger {
  info(event: string, fields?: Record<string, unknown>): void;
  error(event: string, fields?: Record<string, unknown>): void;
  debug(event: string, fields?: Record<string, unknown>): void;
}

export function createLogger(verbose = false): Logger {
  const write = (level: string, event: string, fields: Record<string, unknown> = {}): void => {
    const line = JSON.stringify({ timestamp: new Date().toISOString(), level, event, ...fields });
    if (level === "error") process.stderr.write(`${line}\n`);
    else process.stdout.write(`${line}\n`);
  };
  return {
    info: (event, fields) => write("info", event, fields),
    error: (event, fields) => write("error", event, fields),
    debug: (event, fields) => { if (verbose) write("debug", event, fields); }
  };
}

export function errorFields(error: unknown): Record<string, unknown> {
  if (error instanceof Error) return { errorName: error.name, errorMessage: error.message };
  return { errorMessage: String(error) };
}

const MASK = '********';

// Match credential-bearing field names used by VS Code, Server Manager, and
// process environments. Keeping this key-based avoids altering benign values.
const SENSITIVE_KEY = /(?:password|passwd|passphrase|access[_-]?token|auth[_-]?token|api[_-]?key|secret)$/i;

/** Return a log-safe copy of a value without modifying the original. */
export function redactSecrets(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(redactSecrets);
  }

  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value).map(([key, nestedValue]) => [
        key,
        SENSITIVE_KEY.test(key) ? MASK : redactSecrets(nestedValue),
      ])
    );
  }

  return value;
}

export function stringifyForLog(value: unknown): string {
  return JSON.stringify(redactSecrets(value));
}

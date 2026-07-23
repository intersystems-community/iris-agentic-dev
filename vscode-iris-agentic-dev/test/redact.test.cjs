const assert = require('node:assert/strict');
const test = require('node:test');

const { redactSecrets, stringifyForLog } = require('../.test-out/redact.cjs');

test('redacts password fields without changing the source object', () => {
  const source = {
    username: 'SuperUser',
    password: 'settings-secret',
    nested: { IRIS_PASSWORD: 'SYS', host: 'localhost' },
  };

  assert.deepEqual(redactSecrets(source), {
    username: 'SuperUser',
    password: '********',
    nested: { IRIS_PASSWORD: '********', host: 'localhost' },
  });
  assert.equal(source.password, 'settings-secret');
  assert.equal(source.nested.IRIS_PASSWORD, 'SYS');
});

test('redacts related credential fields in nested objects and arrays', () => {
  const logValue = stringifyForLog({
    accessToken: 'token-value',
    servers: [{ api_key: 'key-value', clientSecret: 'secret-value' }],
  });

  assert.equal(logValue.includes('token-value'), false);
  assert.equal(logValue.includes('key-value'), false);
  assert.equal(logValue.includes('secret-value'), false);
  assert.equal(logValue.includes('********'), true);
});

test('keeps non-sensitive launch environment values visible', () => {
  assert.equal(
    stringifyForLog({ IRIS_HOST: 'localhost', IRIS_WEB_PORT: 8080 }),
    '{"IRIS_HOST":"localhost","IRIS_WEB_PORT":8080}'
  );
});

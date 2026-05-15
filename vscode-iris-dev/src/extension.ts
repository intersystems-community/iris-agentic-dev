import * as vscode from 'vscode';
import * as os from 'os';
import * as fs from 'fs';
import * as path from 'path';
import which from 'which';
import * as serverManager from '@intersystems-community/intersystems-servermanager';

function findIrisDev(): string | null {
  const cfg = vscode.workspace.getConfiguration('iris-dev');
  const override = cfg.get<string>('serverPath');
  if (override) { return override; }
  // Try plain name first, then .exe for Windows
  for (const name of ['iris-dev', 'iris-dev.exe']) {
    try { return which.sync(name); } catch { /* try next */ }
  }
  return null;
}

interface ObjectScriptConn {
  active: boolean;
  host?: string;
  port?: number;
  ns?: string;
  username?: string;
  password?: string;
  server?: string;
}

interface NamedServer {
  webServer: {
    host?: string;
    port?: number;
    scheme?: string;
    pathPrefix?: string;
  };
  superServer?: { host?: string; port: number; };
  ns?: string;
  username?: string;
  password?: string;
}

export class IrisDevMcpProvider
  implements vscode.McpServerDefinitionProvider<vscode.McpStdioServerDefinition>, vscode.Disposable
{
  private readonly emitter = new vscode.EventEmitter<void>();
  private readonly watcher: vscode.Disposable;
  private readonly log = vscode.window.createOutputChannel('iris-dev', { log: true });

  public readonly onDidChangeMcpServerDefinitions = this.emitter.event;

  constructor() {
    this.watcher = vscode.workspace.onDidChangeConfiguration(e => {
      if (
        e.affectsConfiguration('objectscript.conn') ||
        e.affectsConfiguration('iris-dev.containerName') ||
        e.affectsConfiguration('iris-dev.nativePort') ||
        e.affectsConfiguration('iris-dev.serverPath') ||
        e.affectsConfiguration('intersystems.servers')
      ) {
        this.emitter.fire();
      }
    });
  }

  dispose() {
    this.watcher.dispose();
    this.emitter.dispose();
    this.log.dispose();
  }

  refresh() { this.emitter.fire(); }

  public provideMcpServerDefinitions(
    _token: vscode.CancellationToken
  ): vscode.ProviderResult<vscode.McpStdioServerDefinition[]> {
    this.log.show(true);   // reveal without stealing focus
    this.log.info('iris-dev: provideMcpServerDefinitions called');

    // Use workspace folder scope to honour .vscode/settings.json (fix #42).
    // Falls back to global scope when no workspace folder is open.
    const wsFolder = vscode.workspace.workspaceFolders?.[0];
    const conn = vscode.workspace
      .getConfiguration('objectscript', wsFolder ?? null)
      .get<ObjectScriptConn>('conn');

    this.log.info(`iris-dev: objectscript.conn = ${JSON.stringify(conn)}`);

    if (!conn || conn.active === false) {
      this.log.warn('iris-dev: ObjectScript connection is not configured or inactive');
      vscode.window.showWarningMessage(
        'iris-dev: ObjectScript connection is not configured or inactive.'
      );
      return [];
    }

    const command = findIrisDev();
    this.log.info(`iris-dev: binary path = ${command ?? '(not found)'}`);
    if (!command) {
      vscode.window.showErrorMessage(
        'iris-dev: binary not found. ' +
        'Download from https://github.com/intersystems-community/iris-dev/releases ' +
        'or set iris-dev.serverPath in VS Code settings.'
      );
      return [];
    }
    const containerName = vscode.workspace
      .getConfiguration('iris-dev')
      .get<string>('containerName');
    this.log.info(`iris-dev: containerName = ${containerName}`);

    const tlsVerify = vscode.workspace
      .getConfiguration('http')
      .get<boolean>('proxyStrictSSL');

    // Resolve named server if using intersystems.servers.
    // Server Manager writes server definitions to user settings, so we must
    // check both workspace-scoped config (for .vscode/settings.json) and
    // global (null) scope. Workspace scope alone misses user-level entries
    // when the workspace has no intersystems config of its own.
    let named: NamedServer | null = null;
    if (conn.server) {
      const wsServers = vscode.workspace
        .getConfiguration('intersystems', wsFolder ?? null)
        .get<Record<string, NamedServer>>('servers');
      const globalServers = vscode.workspace
        .getConfiguration('intersystems', null)
        .get<Record<string, NamedServer>>('servers');
      this.log.info(`iris-dev: globalServers keys = ${Object.keys(globalServers ?? {}).join(', ') || '(none)'}`);
      this.log.info(`iris-dev: wsServers keys = ${Object.keys(wsServers ?? {}).join(', ') || '(none)'}`);
      const servers = { ...globalServers, ...wsServers };
      this.log.info(`iris-dev: looking for server "${conn.server}" in merged servers: ${Object.keys(servers).join(', ') || '(none)'}`);
      if (!servers || !servers[conn.server]) {
        this.log.warn(`iris-dev: named connection "${conn.server}" not found`);
        vscode.window.showWarningMessage(
          `iris-dev: named connection "${conn.server}" not found in intersystems.servers. Check your .vscode/settings.json or user settings.`
        );
        return [];
      }
      named = servers[conn.server];
      this.log.info(`iris-dev: named server resolved = ${JSON.stringify(named)}`);
    }

    const host = conn.host ?? 'localhost';
    const webPort = conn.port ?? 52773;
    const namespace = conn.ns ?? 'USER';

    const resolvedHost = (named?.superServer?.host ?? named?.webServer?.host) ?? host;
    const webPrefix = named?.webServer?.pathPrefix ?? null;
    const webScheme = named?.webServer?.scheme ?? null;

    const isIsfs = vscode.workspace.workspaceFolders?.some(
      f => f.uri.scheme === 'isfs' || f.uri.scheme === 'isfs-readonly'
    ) ?? false;

    if (resolvedHost !== 'localhost' && resolvedHost !== '127.0.0.1' && resolvedHost !== '::1') {
      vscode.window.showWarningMessage(
        `iris-dev: connected to remote IRIS host "${resolvedHost}". ` +
        'Recommended: use a local or dedicated dev instance.'
      );
    }

    // Build env — omit undefined/null values so Windows process spawning doesn't choke
    const envRaw: Record<string, string | number | undefined> = {
      IRIS_HOST: resolvedHost,
      IRIS_WEB_PORT: named?.webServer?.port ?? webPort,
      IRIS_WEB_PREFIX: webPrefix ?? undefined,
      IRIS_SCHEME: webScheme ?? undefined,
      IRIS_USERNAME: named?.username ?? conn.username ?? undefined,
      IRIS_PASSWORD: named?.password ?? conn.password ?? undefined,
      IRIS_NAMESPACE: named?.ns ?? namespace,
      IRIS_ISFS: isIsfs ? 'true' : undefined,
      IRIS_SERVER_NAME: conn.server ?? undefined,
      IRIS_CONTAINER: containerName ?? undefined,
      IRIS_TLS_VERIFY: tlsVerify ? 'true' : 'false',
      OBJECTSCRIPT_LEARNING: 'true',
    };
    const env: Record<string, string | number> = Object.fromEntries(
      Object.entries(envRaw).filter(([, v]) => v !== undefined && v !== null)
    ) as Record<string, string | number>;

    this.log.info(`iris-dev: scheme=${webScheme ?? 'http'} prefix=${webPrefix ?? '(none)'}`);
    this.log.info(`iris-dev: launching binary with env = ${JSON.stringify(env)}`);

    const definition = new vscode.McpStdioServerDefinition(
      'iris-dev (IRIS)',
      command,
      ['mcp']           // iris-dev requires the "mcp" subcommand
    );
    definition.env = env;
    return [definition];
  }

  public async resolveMcpServerDefinition(
    server: vscode.McpStdioServerDefinition,
    token: vscode.CancellationToken
  ): Promise<vscode.McpStdioServerDefinition | undefined> {
    if (token.isCancellationRequested || !(server instanceof vscode.McpStdioServerDefinition)) {
      return server;
    }
    const env: Record<string, string | number> = { ...(server.env ?? {}) } as Record<string, string | number>;
    if (!env.IRIS_PASSWORD) {
      const namedServer = env.IRIS_SERVER_NAME as string | undefined;
      let resolvedByServerManager = false;

      // Try InterSystems Server Manager authentication provider when a named server is configured
      if (namedServer) {
        const smExt = vscode.extensions.getExtension<serverManager.ServerManagerAPI>(serverManager.EXTENSION_ID);
        if (smExt) {
          try {
            if (!smExt.isActive) {
              await smExt.activate();
            }
            const api = smExt.exports;
            if (api?.getServerSpec) {
              const spec = await api.getServerSpec(namedServer);
              if (spec) {
                if (typeof spec.password !== 'undefined') {
                  // Password stored in settings (deprecated) — use it directly
                  env.IRIS_PASSWORD = spec.password;
                  server.env = env;
                  resolvedByServerManager = true;
                } else {
                  const scopes = [spec.name, spec.username || ''];
                  const account = api.getAccount?.(spec);
                  const sessionOptions = account ? { account } : {};
                  let session = await vscode.authentication.getSession(
                    serverManager.AUTHENTICATION_PROVIDER, scopes, { silent: true, ...sessionOptions }
                  );
                  if (!session) {
                    session = await vscode.authentication.getSession(
                      serverManager.AUTHENTICATION_PROVIDER, scopes, { createIfNone: true, ...sessionOptions }
                    );
                  }
                  if (session) {
                    const username = session.scopes[1]?.toLowerCase() === 'unknownuser' ? '' : session.scopes[1];
                    if (username) { env.IRIS_USERNAME = username; }
                    env.IRIS_PASSWORD = session.accessToken;
                    server.env = env;
                    resolvedByServerManager = true;
                  }
                }
              }
            }
          } catch (err) {
            this.log.warn(`iris-dev: Server Manager credential lookup failed: ${err}`);
          }
        }
      }

      // Fall back to a password prompt if Server Manager did not provide credentials
      if (!resolvedByServerManager) {
        const pw = await vscode.window.showInputBox({ prompt: 'IRIS password', password: true });
        if (pw !== undefined) { env.IRIS_PASSWORD = pw; server.env = env; }
      }
    }
    return server;
  }
}

function hasIsfsWorkspace(): boolean {
  return (vscode.workspace.workspaceFolders ?? []).some(
    f => f.uri.scheme === 'isfs' || f.uri.scheme === 'isfs-readonly'
  );
}

function setupOpenHintWatcher(context: vscode.ExtensionContext): void {
  const hintDir = path.join(os.homedir(), '.iris-dev');
  const hintPath = path.join(hintDir, 'open-hint.json');

  // Create dir if needed
  try { fs.mkdirSync(hintDir, { recursive: true }); } catch {}

  const pattern = new vscode.RelativePattern(hintDir, 'open-hint.json');
  const watcher = vscode.workspace.createFileSystemWatcher(pattern);

  const openFromHint = async () => {
    try {
      if (!hasIsfsWorkspace()) { return; }
      const raw = fs.readFileSync(hintPath, 'utf8');
      const hint = JSON.parse(raw) as { uri: string; ts: number };
      if (Date.now() - hint.ts < 3000) {
        await vscode.window.showTextDocument(vscode.Uri.parse(hint.uri), { preview: false });
      }
    } catch {
      // Silently ignore — file may not exist or workspace isn't ISFS
    }
  };

  watcher.onDidChange(openFromHint);
  watcher.onDidCreate(openFromHint);
  context.subscriptions.push(watcher);
}

export function activate(context: vscode.ExtensionContext): void {
  const provider = new IrisDevMcpProvider();
  context.subscriptions.push(provider);

  setupOpenHintWatcher(context);

  if (typeof vscode.lm?.registerMcpServerDefinitionProvider === 'function') {
    context.subscriptions.push(
      vscode.lm.registerMcpServerDefinitionProvider('iris-dev', provider)
    );
    provider.refresh();
  } else {
    vscode.window.showWarningMessage(
      'iris-dev: MCP server registration requires VS Code 1.99+.'
    );
  }
}

export function deactivate(): void {}

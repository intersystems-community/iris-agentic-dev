// Which `intersystems.servers` definition a connection uses, and whether it
// may receive a stored password.
//
// VS Code merges user and workspace settings, and a workspace file comes with
// the repository. If the workspace definition of "prod" won over the user's,
// opening a cloned repository would point "prod" at the repository's host and
// the extension would then hand it the password the user saved for the real
// "prod". So the user's definition wins, and a definition that exists only in
// the workspace never gets a stored password (it may carry its own).

export type ServerSource = "user" | "workspace";

export interface ServerScopes<T> {
  /** `intersystems.servers` from the user's settings (and remote machine settings). */
  user: Record<string, T> | undefined;
  /** The same setting from the workspace and workspace folder, folder over workspace. */
  workspace: Record<string, T> | undefined;
}

export interface SelectedServer<T> {
  name: string;
  server: T;
  source: ServerSource;
  /** A workspace definition of the same name exists and was ignored. */
  shadowedWorkspaceDefinition: boolean;
}

function own<T>(map: Record<string, T> | undefined, name: string): T | undefined {
  if (!map || typeof map !== "object") return undefined;
  return Object.prototype.hasOwnProperty.call(map, name) ? map[name] : undefined;
}

export function selectNamedServer<T>(
  name: string,
  scopes: ServerScopes<T>
): SelectedServer<T> | null {
  const fromUser = own(scopes.user, name);
  const fromWorkspace = own(scopes.workspace, name);
  if (fromUser) {
    return {
      name,
      server: fromUser,
      source: "user",
      shadowedWorkspaceDefinition: fromWorkspace !== undefined,
    };
  }
  if (fromWorkspace) {
    return { name, server: fromWorkspace, source: "workspace", shadowedWorkspaceDefinition: false };
  }
  return null;
}

/** The stored (Server Manager) password belongs to the user's definition only. */
export function mayUseStoredPassword(selected: { source: ServerSource }): boolean {
  return selected.source === "user";
}

/** The `objectscript.conn` fields that decide where a password may go. */
export interface ConnLike {
  host?: string;
  port?: number;
  server?: string;
  username?: string;
  password?: string;
}

/** One setting as `WorkspaceConfiguration.inspect` reports it, layer by layer. */
export interface ConnLayers<T> {
  defaultValue?: T;
  globalValue?: T;
  workspaceValue?: T;
  workspaceFolderValue?: T;
}

/**
 * A setting's value as the user set it: the user layer, else the default.
 * `get()` would let a workspace's `.vscode/settings.json` override it, and
 * these settings decide whether the connection is encrypted and verified.
 */
export function userLevelValue<T>(layers: ConnLayers<T> | undefined): T | undefined {
  return layers?.globalValue ?? layers?.defaultValue;
}

/**
 * The effective `objectscript.conn`: the layers merged, workspace over user,
 * as `get()` returns them. Undefined when no layer sets it.
 */
export function mergeConn<T extends object>(layers: ConnLayers<T> | undefined): T | undefined {
  if (!layers) return undefined;
  const set = [
    layers.defaultValue,
    layers.globalValue,
    layers.workspaceValue,
    layers.workspaceFolderValue,
  ].filter((l): l is T => l !== undefined && l !== null);
  return set.length === 0 ? undefined : Object.assign({}, ...set);
}

const DEFAULT_HOST = "localhost";
const DEFAULT_PORT = 52773;

function sameTarget(a: ConnLike, b: ConnLike): boolean {
  const host = (c: ConnLike) => (c.host ?? DEFAULT_HOST).trim().toLowerCase();
  return (
    host(a) === host(b) &&
    (a.port ?? DEFAULT_PORT) === (b.port ?? DEFAULT_PORT) &&
    (a.server ?? "") === (b.server ?? "") &&
    (a.username ?? "").trim().toLowerCase() === (b.username ?? "").trim().toLowerCase()
  );
}

/**
 * The password `objectscript.conn` may hand to the server it resolves to.
 *
 * `get()` merges the user's settings with the workspace's, and a workspace
 * file comes with the repository. A workspace that sets only `host` and
 * `port` would then be sent the password the user saved for their own server.
 * So:
 *
 * - a password written in the workspace's `conn` belongs to that file and
 *   goes where the file says (the same terms as a workspace server
 *   definition that carries its own password);
 * - the user-level password goes only when the effective host, port,
 *   server name and username equal the user-level ones, and never to a server defined
 *   only in the workspace (`namedFromWorkspace`).
 *
 * Anything else gets no password; the caller asks the user.
 */
export function connPassword<T extends ConnLike>(
  layers: ConnLayers<T> | undefined,
  namedFromWorkspace: boolean
): string | undefined {
  const effective = mergeConn(layers);
  if (!effective) return undefined;
  const fromWorkspace = Object.assign(
    {},
    layers?.workspaceValue,
    layers?.workspaceFolderValue
  ) as ConnLike;
  if (fromWorkspace.password) return fromWorkspace.password;
  const user = mergeConn({ defaultValue: layers?.defaultValue, globalValue: layers?.globalValue });
  if (!user?.password || namedFromWorkspace) return undefined;
  return sameTarget(effective, user) ? user.password : undefined;
}

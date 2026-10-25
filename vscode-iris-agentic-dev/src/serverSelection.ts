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

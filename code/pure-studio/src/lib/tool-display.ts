import type { ToolCallStatus2 } from "../types";

export function isQuietFileTool(name: string | null | undefined): boolean {
  return matchesToolName(name, [
    "read_file",
    "write_file",
    "list_files",
    "list_file",
    "search_files",
    "stat_path",
    "create_directory",
    "delete_path",
    "copy_path",
    "move_path",
    "apply_patch",
  ]);
}

export function hidesToolResult(
  name: string | null | undefined,
  status: ToolCallStatus2 | null | undefined,
): boolean {
  if (status !== "completed") {
    return false;
  }
  return matchesToolName(name, ["read_file", "list_files", "list_file", "search_files", "stat_path"]);
}

function matchesToolName(name: string | null | undefined, names: string[]): boolean {
  const normalized = name?.toLowerCase();
  return Boolean(normalized && names.includes(normalized));
}

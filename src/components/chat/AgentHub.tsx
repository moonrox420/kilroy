/**
 * Agent hub — Kilroy's unified agent chat surface.
 *
 * SmartCoder is Kilroy's default agent (`code_agent` mode) inside AgentChat —
 * not a separate tab or panel.
 */
import { AgentChat } from "./AgentChat";

export function AgentHub() {
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="min-h-0 flex-1">
        <AgentChat />
      </div>
    </div>
  );
}
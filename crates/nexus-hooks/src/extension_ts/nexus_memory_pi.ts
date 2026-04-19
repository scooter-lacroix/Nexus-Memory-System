/**
 * Nexus Memory System — Pi-Mono Extension
 *
 * Automatically captures session context and stores memories via the Nexus CLI.
 * Installed by the Nexus hooks system. Do not edit manually.
 *
 * @version 1.0.0
 * @see https://github.com/scooter-lacroix/nexus-memory-system
 */

import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";
import { execSync, spawn } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import * as crypto from "node:crypto";

export default function nexusMemory(pi: ExtensionAPI): void {
  const AGENT_TYPE = "pi-mono";
  const NEXUS_BIN = process.env.NEXUS_HOOK_BINARY || findNexusBinary();

  // Subconscious mode: skip all subconscious calls when 'off'
  const SUBCONSCIOUS_MODE = (process.env.NEXUS_SUBCONSCIOUS_MODE || "whisper").toLowerCase();
  const SUBCONSCIOUS_OFF = SUBCONSCIOUS_MODE === "off";

  // Session state (scoped to this extension instance)
  let sessionId: string | null = null;
  let sessionCwd: string | null = null;
  let lastIngestedContent: string | null = null;
  let ingestQueue: NexusPayload[] = [];
  let lastIngestTime = 0;
  let flushTimer: ReturnType<typeof setTimeout> | null = null;
  const INGEST_DEBOUNCE_MS = 2000;
  const MAX_QUEUE_SIZE = 100;

  // ── Lifecycle Events ───────────────────────────────────────────

  pi.on("session_start", async (event, ctx) => {
    try {
      // Flush any pending debounce state before resetting
      if (flushTimer) {
        clearTimeout(flushTimer);
        flushTimer = null;
      }
      if (ingestQueue.length > 0) {
        await flushQueue();
      }
      lastIngestTime = 0;

      sessionId = deriveSessionId(ctx);
      sessionCwd = ctx.cwd;
      lastIngestedContent = null;
      ingestQueue = [];

      await spawnNexus([
        "session", "start",
        "--agent", AGENT_TYPE,
        "--session-key", sessionId,
        "--cwd", ctx.cwd,
        "--mode", "session",
      ]);

      // Subconscious: initialize memory retrieval for this session
      if (!SUBCONSCIOUS_OFF) {
        await spawnNexus([
          "subconscious", "session-start",
          "--cwd", ctx.cwd,
        ]);
      }
    } catch (err) {
      logError("session_start", err);
    }
  });

  pi.on("session_shutdown", async (_event, ctx) => {
    try {
      // Flush pending queue
      await flushQueue();

      if (sessionId) {
        await spawnNexus([
          "session", "end",
          "--agent", AGENT_TYPE,
          "--session-key", sessionId,
          "--cwd", sessionCwd || ctx.cwd,
          "--reason", "session_shutdown",
        ]);
      }
    } catch (err) {
      logError("session_shutdown", err);
    }
  });

  pi.on("session_compact", async (_event, ctx) => {
    try {
      if (sessionId) {
        await spawnNexus([
          "session", "event",
          "--agent", AGENT_TYPE,
          "--session-key", sessionId,
          "--cwd", sessionCwd || ctx.cwd,
          "--kind", "compact",
        ]);
      }
    } catch (err) {
      logError("session_compact", err);
    }
  });

  // ── Content Events (Ingestion) ─────────────────────────────────

  pi.on("tool_result", async (event, ctx) => {
    try {
      const contentText = extractTextContent(event.content);
      if (!contentText && !event.isError) return; // Skip trivial results

      const payload: NexusPayload = {
        agent: AGENT_TYPE,
        event_name: "tool_result",
        session_id: sessionId,
        cwd: sessionCwd || ctx.cwd,
        tool_name: event.toolName,
        tool_input: event.input,
        tool_response_text: contentText || null,
        assistant_message_text: null,
        user_message_text: null,
      };

      await throttledIngest(payload);

      // Synthetic error event for failed tools
      if (event.isError && sessionId) {
        await spawnNexus([
          "session", "event",
          "--agent", AGENT_TYPE,
          "--session-key", sessionId,
          "--cwd", sessionCwd || ctx.cwd,
          "--kind", "error",
        ]);
      }
    } catch (err) {
      logError("tool_result", err);
    }
  });

  pi.on("agent_end", async (event, ctx) => {
    try {
      const messages = event.messages || [];
      const lastAssistant = messages.filter(m => m.role === "assistant").pop();
      const lastUser = messages.filter(m => m.role === "user").pop();

      let assistantText: string | null = null;
      let userText: string | null = null;

      if (lastAssistant && "content" in lastAssistant) {
        assistantText = extractMessageText(lastAssistant.content);
      }
      if (lastUser && "content" in lastUser) {
        userText = typeof lastUser.content === "string"
          ? lastUser.content
          : extractMessageText(lastUser.content);
      }

      if (assistantText || userText) {
        const payload: NexusPayload = {
          agent: AGENT_TYPE,
          event_name: "agent_end",
          session_id: sessionId,
          cwd: sessionCwd || ctx.cwd,
          tool_name: null,
          tool_input: null,
          tool_response_text: null,
          assistant_message_text: assistantText,
          user_message_text: userText,
        };

        await throttledIngest(payload);
      }

      // Synthetic error for abnormal agent_end
      if (lastAssistant && "stopReason" in lastAssistant) {
        const msg = lastAssistant as any;
        if (msg.stopReason === "error" || msg.errorMessage) {
          if (sessionId) {
            await spawnNexus([
              "session", "event",
              "--agent", AGENT_TYPE,
              "--session-key", sessionId,
              "--cwd", sessionCwd || ctx.cwd,
              "--kind", "error",
            ]);
          }
        }
      }

      // Emit checkpoint after agent_end
      if (sessionId) {
        await spawnNexus([
          "session", "event",
          "--agent", AGENT_TYPE,
          "--session-key", sessionId,
          "--cwd", sessionCwd || ctx.cwd,
          "--kind", "checkpoint",
        ]);
      }
    } catch (err) {
      logError("agent_end", err);
    }
  });

  pi.on("message_end", async (event, ctx) => {
    try {
      if (event.message.role !== "assistant") return;

      const content = "content" in event.message ? event.message.content : null;
      if (!content) return;

      const text = extractMessageText(content);
      if (!text) return;

      // Debounce: skip if identical to last ingested content
      if (text === lastIngestedContent) return;
      lastIngestedContent = text;

      const payload: NexusPayload = {
        agent: AGENT_TYPE,
        event_name: "message_end",
        session_id: sessionId,
        cwd: sessionCwd || ctx.cwd,
        tool_name: null,
        tool_input: null,
        tool_response_text: null,
        assistant_message_text: text,
        user_message_text: null,
      };

      await throttledIngest(payload);
    } catch (err) {
      logError("message_end", err);
    }
  });

  // ── Context Injection ──────────────────────────────────────────

  pi.on("before_agent_start", async (event, ctx) => {
    try {
      // Primary: live subconscious recall (queries cognitive cache + soul.md)
      let subconsciousContext: string | null = null;
      if (!SUBCONSCIOUS_OFF && sessionId) {
        subconsciousContext = await spawnNexusCapture([
          "subconscious", "recall",
          "--session-id", sessionId,
          "--cwd", sessionCwd || ctx.cwd,
        ], event.userMessage || "");
      }

      // Fallback: static context.md (legacy)
      let staticContext: string | null = null;
      const contextPath = path.join(ctx.cwd, ".nexus", "context.md");
      if (fs.existsSync(contextPath)) {
        staticContext = fs.readFileSync(contextPath, "utf-8").trim() || null;
      }

      const parts: string[] = [];
      if (subconsciousContext) {
        parts.push("## Nexus Subconscious\n\n" + subconsciousContext);
      }
      if (staticContext) {
        parts.push("## Nexus Memory Context\n\n" + staticContext);
      }

      if (parts.length > 0) {
        return {
          systemPrompt: event.systemPrompt + "\n\n" + parts.join("\n\n"),
        };
      }
    } catch {
      // Silently skip if subconscious retrieval fails
    }
  });

  // ── Helpers ────────────────────────────────────────────────────

  interface NexusPayload {
    agent: string;
    event_name: string;
    session_id: string | null;
    cwd: string | null;
    tool_name: string | null;
    tool_input: unknown | null;
    tool_response_text: string | null;
    assistant_message_text: string | null;
    user_message_text: string | null;
  }

  function deriveSessionId(ctx: ExtensionContext): string {
    const sessionFile = ctx.sessionManager.getSessionFile?.();
    if (sessionFile) {
      return path.basename(sessionFile, path.extname(sessionFile));
    }
    return crypto.randomUUID();
  }

  function findNexusBinary(): string {
    const candidates = [
      path.join(process.env.HOME || process.env.USERPROFILE || os.homedir() || ".", ".local", "bin", "nexus"),
      "/usr/local/bin/nexus",
    ];
    for (const c of candidates) {
      if (fs.existsSync(c)) return c;
    }
    return "nexus";
  }

  function extractTextContent(content: any[]): string | null {
    if (!Array.isArray(content)) return null;
    const texts = content
      .filter((c: any) => c.type === "text" && c.text)
      .map((c: any) => c.text);
    return texts.length > 0 ? texts.join("\n").slice(0, 10000) : null;
  }

  function extractMessageText(content: any): string | null {
    if (typeof content === "string") return content.slice(0, 10000);
    if (!Array.isArray(content)) return null;
    return extractTextContent(content);
  }

  async function throttledIngest(payload: NexusPayload): Promise<void> {
    const now = Date.now();
    if (now - lastIngestTime < INGEST_DEBOUNCE_MS) {
      // Queue for later
      if (ingestQueue.length < MAX_QUEUE_SIZE) {
        ingestQueue.push(payload);
      }
      // Schedule a flush if not already pending
      if (!flushTimer) {
        const delay = INGEST_DEBOUNCE_MS - (now - lastIngestTime);
        flushTimer = setTimeout(async () => {
          flushTimer = null;
          if (ingestQueue.length > 0) {
            const queued = ingestQueue.splice(0);
            for (const p of queued) {
              await ingestPayload(p);
            }
          }
        }, delay);
      }
      return;
    }

    lastIngestTime = now;
    // Cancel any pending flush since we're flushing now
    if (flushTimer) {
      clearTimeout(flushTimer);
      flushTimer = null;
    }

    // Flush any queued items first
    if (ingestQueue.length > 0) {
      const queued = ingestQueue.splice(0);
      for (const p of queued) {
        await ingestPayload(p);
      }
    }

    await ingestPayload(payload);
  }

  async function ingestPayload(payload: NexusPayload): Promise<void> {
    const json = JSON.stringify(payload);
    const args = ["ingest-hook-event"];
    if (payload.agent) args.push("--agent", payload.agent);
    if (payload.event_name) args.push("--event", payload.event_name);
    await spawnNexus(args, json);
  }

  async function flushQueue(): Promise<void> {
    if (ingestQueue.length === 0) return;
    const queued = ingestQueue.splice(0);
    for (const p of queued) {
      await ingestPayload(p);
    }
  }

  function spawnNexus(args: string[], stdinData?: string): Promise<void> {
    return new Promise((resolve) => {
      try {
        const child = spawn(NEXUS_BIN, args, {
          stdio: stdinData ? ["pipe", "ignore", "ignore"] : ["ignore", "ignore", "ignore"],
          detached: true,
          env: { ...process.env },
        });

        if (stdinData && child.stdin) {
          child.stdin.write(stdinData);
          child.stdin.end();
        }

        child.unref();

        const timeout = setTimeout(() => resolve(), 5000);
        child.on("exit", () => { clearTimeout(timeout); resolve(); });
        child.on("error", () => { clearTimeout(timeout); resolve(); });
      } catch {
        resolve(); // Never throw
      }
    });
  }

  /**
   * Run a nexus command and capture stdout. Used by subconscious recall
   * to retrieve XML memory context for injection into the session.
   */
  async function spawnNexusCapture(args: string[], stdinData?: string): Promise<string | null> {
    return new Promise((resolve) => {
      try {
        const child = spawn(NEXUS_BIN, args, {
          stdio: stdinData ? ["pipe", "pipe", "ignore"] : ["ignore", "pipe", "ignore"],
          env: { ...process.env },
        });

        if (stdinData && child.stdin) {
          child.stdin.write(stdinData);
          child.stdin.end();
        }

        let output = "";
        if (child.stdout) {
          child.stdout.on("data", (data: Buffer) => {
            output += data.toString();
          });
        }

        const timeout = setTimeout(() => {
          child.kill();
          resolve(output.trim() || null);
        }, 10000);

        child.on("exit", () => {
          clearTimeout(timeout);
          resolve(output.trim() || null);
        });
        child.on("error", () => {
          clearTimeout(timeout);
          resolve(null);
        });
      } catch {
        resolve(null); // Never throw
      }
    });
  }
  function logError(event: string, err: unknown): void {
    const msg = err instanceof Error ? err.message : String(err);
    console.error(`[nexus-memory] Error in ${event}: ${msg}`);
  }
}

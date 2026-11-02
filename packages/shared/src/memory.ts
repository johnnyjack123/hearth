import { z } from 'zod';

/** Tool of chats and agents that stores a memory for all chats. */
export const REMEMBER_TOOL = 'remember';

/** Tool of chats and agents that removes one. */
export const FORGET_TOOL = 'forget';

/** Memories a user keeps at most; each one goes into every chat. */
export const MAX_MEMORIES = 50;

/** Longest memory in characters. */
export const MAX_MEMORY_CHARS = 300;

/** Who stored a memory: the user in the settings, a chat model or an agent. */
export type MemorySource = 'user' | 'chat' | 'agent';

/**
 * Something the user wants every chat and agent to know, such as "answer
 * briefly" or "pnpm instead of npm".
 */
export interface Memory {
  /** Memory id (UUID). */
  id: string;
  /** What to remember. */
  text: string;
  /** Who stored it. */
  source: MemorySource;
  /** The chat it came from, if a model stored it there. */
  conversationId: string | null;
  /** Creation time, epoch milliseconds. */
  createdAt: number;
  /** Time of the last change, epoch milliseconds. */
  updatedAt: number;
}

/** Validates the text of a memory. */
export const memoryTextSchema = z.string().trim().min(1).max(MAX_MEMORY_CHARS);

/** Validates a memory the user adds or changes. */
export const memoryInputSchema = z.object({ text: memoryTextSchema }).strict();

/** See {@link memoryInputSchema}. */
export type MemoryInput = z.infer<typeof memoryInputSchema>;

/**
 * Output of {@link REMEMBER_TOOL} and {@link FORGET_TOOL}: the memory that
 * was stored or removed, shown in the chat.
 */
export interface MemoryToolOutput {
  /** Short id the model uses, see {@link memoryRef}. */
  ref: string;
  /** The memory's text. */
  text: string;
}

/**
 * The short id models see and pass to {@link FORGET_TOOL}: the end of the
 * UUID, which is random (the start is a timestamp).
 *
 * @param id - Memory id.
 * @returns Eight hex digits.
 */
export function memoryRef(id: string): string {
  return id.slice(-8);
}

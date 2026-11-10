import {
  type AppKind,
  type AppTemplate,
  type CreateAppTemplateInput,
  type DeviceAppTemplate,
  MAX_APP_TEMPLATES,
  type UpdateAppTemplateInput,
} from '@hearth/shared';
import type { Kysely, Selectable } from 'kysely';
import { v7 as uuidv7 } from 'uuid';
import type { AppTemplateTable, DB } from '../db/schema.ts';
import { HttpError, notFound } from '../http/errors.ts';

type TemplateRow = Selectable<AppTemplateTable>;

/**
 * A stored list column; a broken one counts as empty.
 *
 * @param json - JSON array of strings.
 * @returns The strings.
 */
function storedList(json: string): string[] {
  try {
    const value: unknown = JSON.parse(json);
    return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : [];
  } catch {
    return [];
  }
}

function toTemplate(row: TemplateRow): AppTemplate {
  return {
    id: row.id,
    name: row.name,
    title: row.title,
    description: row.description,
    kind: row.kind as AppKind,
    packages: storedList(row.packages),
    script: row.script,
    hosts: storedList(row.hosts),
    path: storedList(row.path),
    requires: storedList(row.requires),
    createdAt: row.createdAt,
    updatedAt: row.updatedAt,
  };
}

/**
 * An own program as devices get it, without the server's bookkeeping.
 *
 * @param template - The stored program.
 * @returns What goes into `apps.templates`.
 */
export function forDevice(template: AppTemplate): DeviceAppTemplate {
  const { name, title, description, kind, packages, script, hosts, path, requires } = template;
  return { name, title, description, kind, packages, script, hosts, path, requires };
}

/**
 * The columns of a program, keeping only what its kind uses: a device must
 * never install from fields the browser no longer shows.
 *
 * @param name - The app name, which may not require itself.
 * @param input - Validated fields.
 * @returns The columns.
 */
function columns(name: string, input: UpdateAppTemplateInput) {
  const packaged = input.kind !== 'script';
  const unique = (items: string[]) => [...new Set(items)];
  if (input.requires.includes(name)) {
    throw new HttpError(400, 'validation', 'A program cannot require itself');
  }
  return {
    title: input.title,
    description: input.description || null,
    kind: input.kind,
    packages: JSON.stringify(packaged ? unique(input.packages) : []),
    script: packaged ? '' : input.script,
    hosts: JSON.stringify(packaged ? [] : unique(input.hosts)),
    path: JSON.stringify(packaged ? [] : unique(input.path)),
    requires: JSON.stringify(unique(input.requires)),
  };
}

/**
 * The own programs of an account, by name.
 *
 * @param db - Open database.
 * @param userId - The account.
 * @returns Its programs.
 */
export async function listAppTemplates(db: Kysely<DB>, userId: string): Promise<AppTemplate[]> {
  const rows = await db.selectFrom('appTemplate').selectAll().where('userId', '=', userId).orderBy('name').execute();
  return rows.map(toTemplate);
}

/**
 * Adds an own program.
 *
 * @param db - Open database.
 * @param userId - The account.
 * @param input - Validated program.
 * @param taken - Names the account's devices already use for apps of
 *   their own (catalog, recipe files); a program of such a name would never
 *   reach the agent, because the computer's app wins.
 * @returns The new program.
 * @throws {HttpError} 409 if the name is taken or the account has too many.
 */
export async function createAppTemplate(
  db: Kysely<DB>,
  userId: string,
  input: CreateAppTemplateInput,
  taken: ReadonlySet<string>,
): Promise<AppTemplate> {
  const existing = await listAppTemplates(db, userId);
  if (existing.length >= MAX_APP_TEMPLATES) {
    throw new HttpError(409, 'too_many_apps', `At most ${MAX_APP_TEMPLATES} own programs`);
  }
  if (taken.has(input.name) || existing.some((template) => template.name === input.name)) {
    throw new HttpError(409, 'app_name_taken', `An app named ${input.name} exists already`);
  }
  const now = Date.now();
  const row: TemplateRow = {
    id: uuidv7(),
    userId,
    name: input.name,
    ...columns(input.name, input),
    createdAt: now,
    updatedAt: now,
  };
  await db.insertInto('appTemplate').values(row).execute();
  return toTemplate(row);
}

/**
 * Changes an own program; its name stays.
 *
 * @param db - Open database.
 * @param userId - The account.
 * @param id - Program id.
 * @param input - Validated fields.
 * @returns The changed program.
 * @throws {HttpError} 404 if the account has no such program.
 */
export async function updateAppTemplate(db: Kysely<DB>, userId: string, id: string, input: UpdateAppTemplateInput): Promise<AppTemplate> {
  const row = await db.selectFrom('appTemplate').selectAll().where('id', '=', id).where('userId', '=', userId).executeTakeFirst();
  if (!row) {
    throw notFound('Program');
  }
  const changed = { ...columns(row.name, input), updatedAt: Date.now() };
  await db.updateTable('appTemplate').set(changed).where('id', '=', id).execute();
  return toTemplate({ ...row, ...changed });
}

/**
 * Deletes an own program. Devices switch it off in their projects when
 * they get the new list.
 *
 * @param db - Open database.
 * @param userId - The account.
 * @param id - Program id.
 * @throws {HttpError} 404 if the account has no such program.
 */
export async function deleteAppTemplate(db: Kysely<DB>, userId: string, id: string): Promise<void> {
  const result = await db.deleteFrom('appTemplate').where('id', '=', id).where('userId', '=', userId).executeTakeFirst();
  if (Number(result.numDeletedRows) !== 1) {
    throw notFound('Program');
  }
}

import { z } from 'zod';
import { appNameSchema } from './agent.ts';

/**
 * How an own program installs: Debian packages, npm packages, Python
 * programs (each in an environment of its own, like pipx), or a script.
 */
export const APP_KINDS = ['apt', 'npm', 'pip', 'script'] as const;

/** See {@link APP_KINDS}. */
export type AppKind = (typeof APP_KINDS)[number];

/**
 * Apps of the catalog shipped with harness (`BUILTIN_APPS` in harness-core
 * config/mod.rs, checked by a test there). An own program cannot take
 * their names: the computer's app would always win.
 */
export const BUILTIN_APP_NAMES = ['c', 'rust', 'go', 'java', 'deno', 'bun', 'sqlite'] as const;

/** Most own programs per account. */
export const MAX_APP_TEMPLATES = 50;

/** Most packages one program names. */
export const MAX_TEMPLATE_PACKAGES = 20;

/** Longest install script. */
export const MAX_TEMPLATE_SCRIPT = 20_000;

// Package names as the device accepts them (harness-core
// config/template.rs): registry names with an optional version, nothing a
// shell or package manager would read as a URL, path or option.
const PACKAGE_PATTERNS: Record<Exclude<AppKind, 'script'>, RegExp> = {
  apt: /^[a-z0-9][a-z0-9+.-]+$/,
  npm: /^(@[A-Za-z0-9][A-Za-z0-9._~-]*\/)?[A-Za-z0-9][A-Za-z0-9._~-]*(@[A-Za-z0-9._^~*+-]+)?$/,
  pip: /^[A-Za-z0-9]([A-Za-z0-9._-]*[A-Za-z0-9])?(\[[A-Za-z0-9._,-]+\])?((==|>=|<=|~=|!=|<|>)[A-Za-z0-9.*+!_-]+)?$/,
};

/**
 * Whether `name` is a package the device installs for `kind`.
 *
 * @param kind - A package kind.
 * @param name - Package name, maybe with a version.
 * @returns Whether it is valid.
 */
export function validPackage(kind: Exclude<AppKind, 'script'>, name: string): boolean {
  return name.length <= 100 && PACKAGE_PATTERNS[kind].test(name);
}

const label = /^(?!-)[a-z0-9-]{1,63}(?<!-)$/;

/**
 * A domain like the gateway's rules take it: `example.com` or
 * `*.example.com`, no IP addresses (`DomainPattern` in harness-gateway).
 */
export const domainPatternSchema = z
  .string()
  .trim()
  .toLowerCase()
  .max(255)
  .refine((text) => {
    const domain = text.startsWith('*.') ? text.slice(2) : text;
    const labels = domain.split('.');
    return labels.length >= 2 && domain.length <= 253 && labels.every((part) => label.test(part)) && /[a-z]/.test(labels.at(-1) ?? '');
  }, 'Expected a domain such as example.com or *.example.com');

/**
 * A folder of a script program for `PATH`: below the program's folder
 * (`bin`) or below the agent's home (`~/.tool/bin`), never above.
 */
export const appPathSchema = z
  .string()
  .max(200)
  .refine((dir) => {
    const relative = dir.startsWith('~/') ? dir.slice(2) : dir;
    return (
      relative.length > 0 &&
      !relative.startsWith('/') &&
      !relative.startsWith('~') &&
      !/[\\:,\n\0]/.test(relative) &&
      relative.split('/').every((part) => part.length > 0 && part !== '..')
    );
  }, 'Expected a folder such as bin');

const templateFields = {
  title: z.string().trim().min(1).max(60),
  description: z.string().trim().max(300).nullable(),
  kind: z.enum(APP_KINDS),
  packages: z.array(z.string().trim()).max(MAX_TEMPLATE_PACKAGES),
  script: z.string().max(MAX_TEMPLATE_SCRIPT),
  hosts: z.array(domainPatternSchema).max(20),
  path: z.array(appPathSchema).max(10),
  requires: z.array(appNameSchema).max(10),
};

/**
 * Checks what only makes sense together: packages that fit their kind, a
 * script for script programs.
 *
 * @param input - The fields.
 * @param context - Where issues go.
 */
function checkKind(input: { kind: AppKind; packages: string[]; script: string }, context: z.RefinementCtx): void {
  const { kind } = input;
  if (kind === 'script') {
    if (input.script.trim() === '') {
      context.addIssue({ code: 'custom', path: ['script'], message: 'A script program needs a script' });
    }
    return;
  }
  if (input.packages.length === 0) {
    context.addIssue({ code: 'custom', path: ['packages'], message: 'Name at least one package' });
  }
  input.packages.forEach((name, index) => {
    if (!validPackage(kind, name)) {
      context.addIssue({ code: 'custom', path: ['packages', index], message: `Not a valid ${kind} package: ${name}` });
    }
  });
}

/**
 * Validates a new own program (`POST /api/app-templates`). The name becomes
 * the app's name on every device and cannot change later.
 */
export const createAppTemplateSchema = z
  .object({ name: appNameSchema, ...templateFields })
  .strict()
  .superRefine(checkKind);

/** See {@link createAppTemplateSchema}. */
export type CreateAppTemplateInput = z.infer<typeof createAppTemplateSchema>;

/** Validates a change of an own program (`PUT /api/app-templates/:id`). */
export const updateAppTemplateSchema = z.object(templateFields).strict().superRefine(checkKind);

/** See {@link updateAppTemplateSchema}. */
export type UpdateAppTemplateInput = z.infer<typeof updateAppTemplateSchema>;

/**
 * An own program as a device gets it (`apps.templates`, protocol 6): the
 * device writes the install script for the package kinds itself.
 */
export interface DeviceAppTemplate {
  /** App name on the devices. */
  name: string;
  /** Name for people. */
  title: string;
  /** One line for listings. */
  description: string | null;
  /** How it installs. */
  kind: AppKind;
  /** Packages of the package kinds, the first names the version. */
  packages: string[];
  /** Install script of a script program. */
  script: string;
  /** Hosts a script program's installer may reach. */
  hosts: string[];
  /** Folders of a script program for `PATH`; `bin` if none. */
  path: string[];
  /** Apps it requires. */
  requires: string[];
}

/** An own program of the account, as the web app lists it. */
export interface AppTemplate extends DeviceAppTemplate {
  /** Template id. */
  id: string;
  /** Creation time, epoch milliseconds. */
  createdAt: number;
  /** Last change, epoch milliseconds. */
  updatedAt: number;
}

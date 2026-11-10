/**
 * The server's program version, set when the image is built
 * (`apps/server/Dockerfile`, from the workspace version in `Cargo.toml`, the
 * same as hearth's); `dev` when running from the sources.
 */
export const SERVER_VERSION = process.env.HEARTH_VERSION?.trim() || 'dev';

/** The commit the image was built from, if known. */
export const SERVER_COMMIT = process.env.HEARTH_COMMIT?.trim() || null;

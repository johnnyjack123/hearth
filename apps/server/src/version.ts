/**
 * The server's program version, set when the image is built
 * (`apps/server/Dockerfile`, from the workspace version in `Cargo.toml`, the
 * same as harness's); `dev` when running from the sources.
 */
export const SERVER_VERSION = process.env.HARNESS_VERSION?.trim() || 'dev';

/** The commit the image was built from, if known. */
export const SERVER_COMMIT = process.env.HARNESS_COMMIT?.trim() || null;

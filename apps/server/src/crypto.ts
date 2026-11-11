import { createCipheriv, createDecipheriv, hkdfSync, randomBytes } from 'node:crypto';

const VERSION = 'v1';
const IV_BYTES = 12;

/**
 * Symmetric encryption for secrets stored in the database.
 */
export interface SecretBox {
  /**
   * Encrypts a value.
   *
   * @param plaintext - Secret to protect.
   * @param context - Associated data the token is bound to (e.g. `userId:rowId`);
   *   decrypting with a different context fails, so tokens cannot be moved
   *   between rows or users.
   * @returns A self-describing token `v1.<iv>.<tag>.<ciphertext>`.
   */
  encrypt(plaintext: string, context: string): string;
  /**
   * Decrypts a token produced by {@link SecretBox.encrypt}.
   *
   * @param token - Encrypted token.
   * @param context - The same associated data used for encryption.
   * @returns The plaintext.
   * @throws {Error} If the token is malformed, tampered with, or the context differs.
   */
  decrypt(token: string, context: string): string;
}

/**
 * Derives a purpose-specific 32-byte key from the server secret, so the auth
 * secret and the encryption key never share key material.
 *
 * @param secret - Server root secret.
 * @param purpose - Label that separates derived keys, e.g. `auth`.
 * @returns 32 bytes of derived key material.
 */
export function deriveKey(secret: string, purpose: string): Buffer {
  return derive(secret, purpose, KEY_SALT);
}

/**
 * Salt of every derived key. It is part of how stored API keys and the auth
 * secret are derived and must never change: it is the project's name from
 * before 0.2.0, kept on purpose. A test pins the derived bytes.
 */
const KEY_SALT = 'llm-harness';

/**
 * The salt 0.2.0 used by mistake (renamed along with the project). API keys
 * stored by that version still decrypt with it.
 */
const SALT_OF_0_2_0 = 'hearth';

function derive(secret: string, purpose: string, salt: string): Buffer {
  return Buffer.from(hkdfSync('sha256', secret, salt, purpose, 32));
}

/**
 * Creates an AES-256-GCM {@link SecretBox} keyed from the server secret.
 *
 * @param secret - Server root secret.
 * @returns The secret box.
 */
export function createSecretBox(secret: string): SecretBox {
  const key = deriveKey(secret, 'provider-api-keys:v1');
  const keyOf020 = derive(secret, 'provider-api-keys:v1', SALT_OF_0_2_0);

  function open(token: string, context: string, material: Buffer): string {
    const [version, iv, tag, ciphertext] = token.split('.');
    if (version !== VERSION || !iv || !tag || ciphertext === undefined) {
      throw new Error('Malformed secret token');
    }
    const decipher = createDecipheriv('aes-256-gcm', material, Buffer.from(iv, 'base64url'));
    decipher.setAAD(Buffer.from(context, 'utf8'));
    decipher.setAuthTag(Buffer.from(tag, 'base64url'));
    return Buffer.concat([decipher.update(Buffer.from(ciphertext, 'base64url')), decipher.final()]).toString('utf8');
  }

  return {
    encrypt(plaintext, context) {
      const iv = randomBytes(IV_BYTES);
      const cipher = createCipheriv('aes-256-gcm', key, iv);
      cipher.setAAD(Buffer.from(context, 'utf8'));
      const ciphertext = Buffer.concat([cipher.update(plaintext, 'utf8'), cipher.final()]);
      const tag = cipher.getAuthTag();
      return [VERSION, iv, tag, ciphertext].map((part) => (typeof part === 'string' ? part : part.toString('base64url'))).join('.');
    },
    decrypt(token, context) {
      try {
        return open(token, context, key);
      } catch (error) {
        try {
          return open(token, context, keyOf020);
        } catch {
          throw error;
        }
      }
    },
  };
}

/**
 * Builds the hint shown instead of a stored API key.
 *
 * @param apiKey - The full key.
 * @returns `…` plus the last four characters, or `…` alone for very short keys.
 */
export function keyHint(apiKey: string): string {
  return apiKey.length > 8 ? `…${apiKey.slice(-4)}` : '…';
}

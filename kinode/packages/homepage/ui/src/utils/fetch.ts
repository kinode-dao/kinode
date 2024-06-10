/**
 * Prepends or strips '/api/' based on the environment.
 * @param {string} path The original path.
 * @return {string} The modified path.
 */
export function getFetchUrl(path: string) {
  const isDevelopment = import.meta.env.DEV;
  if (isDevelopment) {
    return `/api${path}`;
  }
  return path.replace(/^\/api/, '');
}
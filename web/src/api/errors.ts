/** The server's structured non-success response. */
export class ApiError extends Error {
  constructor(
    public code: string,
    message: string,
    public status: number,
    public details: unknown = null,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}
export type Transport = <T>(path: string, init?: RequestInit) => Promise<T>;

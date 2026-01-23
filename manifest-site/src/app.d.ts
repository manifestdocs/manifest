// See https://svelte.dev/docs/kit/types#app.d.ts
// for information about these interfaces

/**
 * User information from authenticated session.
 */
interface User {
	id: string;
	email: string;
	displayName?: string;
	avatarUrl?: string;
}

/**
 * Session information for authenticated users.
 */
interface Session {
	id: string;
	user: User;
	expiresAt: string;
}

/**
 * Custom error response format.
 */
interface CustomError {
	message: string;
	code: string;
	stack?: string;
}

declare global {
	namespace App {
		interface Error extends CustomError {}

		interface Locals {
			/** Authenticated user (null in local mode or when not logged in) */
			user: User | null;
			/** Session details (null in local mode or when not logged in) */
			session: Session | null;
		}

		interface PageData {
			/** User from server locals */
			user?: User | null;
		}

		// interface PageState {}
		// interface Platform {}
	}
}

export {};

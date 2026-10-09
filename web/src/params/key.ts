import type { ParamMatcher } from '@sveltejs/kit';

/** The names the app's own routes and files take, which no repo's key may be. */
const RESERVED = new Set(['settings', 'jobs', 'api', 'assets', '_app', 'fonts', 'export']);

/** A repo's key as lattice makes it: lowercase letters, digits and dashes. */
export const match: ParamMatcher = (param) => /^[a-z0-9][a-z0-9-]*$/.test(param) && !RESERVED.has(param);

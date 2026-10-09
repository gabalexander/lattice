import type { ParamMatcher } from '@sveltejs/kit';

/** A version's or a job's number. */
export const match: ParamMatcher = (param) => /^[1-9]\d{0,8}$/.test(param);

// Cross-import API (FSD `@x`) for entities/session: a session embeds the
// authenticated user, so the session entity may reference the User type. This is
// the one sanctioned same-layer import — everyone else reaches user through its
// normal public API.

export type { User } from "../model/user";

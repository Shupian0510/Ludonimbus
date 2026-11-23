/** English plural helper: `plural(n, "game", "games")`. */
export function plural(n: number, one: string, many: string): string {
  return n === 1 ? one : many;
}

/**
 * PascalCase 또는 camelCase 문자열을 kebab-case로 변환한다.
 *
 * @example
 * toKebabCase('DailySettlementJob') // 'daily-settlement-job'
 * toKebabCase('myStep')             // 'my-step'
 */
export function toKebabCase(name: string): string {
  return name
    .replace(/([A-Z])/g, '-$1')
    .toLowerCase()
    .replace(/^-/, '');
}

export function parseStrk(value: string): string {
  const trimmed=value.trim();
  if(!/^\d+(\.\d{1,18})?$/.test(trimmed))throw new Error("Enter a positive STRK amount with at most 18 decimals.");
  const [whole,fraction=""]=trimmed.split(".");
  const amount=BigInt(whole)*10n**18n+BigInt(fraction.padEnd(18,"0"));
  if(amount<=0n||amount>=1n<<128n)throw new Error("STRK amount is out of range.");
  return amount.toString();
}
export function formatStrk(value: string): string {
  const n=BigInt(value),whole=n/10n**18n,fraction=(n%10n**18n).toString().padStart(18,"0");
  const visible=fraction.slice(0,8).replace(/0+$/,"");
  return `${whole}${visible?"."+visible:""}${fraction.slice(8).match(/[1-9]/)?"…":""}`;
}

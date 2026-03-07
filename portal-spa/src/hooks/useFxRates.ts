import { useQuery } from "@tanstack/react-query";

/** CoinGecko ID mapping for crypto currencies */
const COINGECKO_IDS: Record<string, string> = {
  BTC: "bitcoin",
  ETH: "ethereum",
  USDT: "tether",
  USDC: "usd-coin"
};

/** Fetch live USD rates for crypto currencies from CoinGecko */
export function useFxRates() {
  const { data: rates = {} } = useQuery({
    queryKey: ["fx-rates"],
    queryFn: async () => {
      const ids = Object.values(COINGECKO_IDS).join(",");
      const resp = await fetch(
        `https://api.coingecko.com/api/v3/simple/price?ids=${ids}&vs_currencies=usd`
      );
      if (!resp.ok) return {};
      const data = await resp.json();
      const result: Record<string, number> = {};
      for (const [currency, geckoId] of Object.entries(COINGECKO_IDS)) {
        const price = data[geckoId]?.usd;
        if (price != null) result[currency] = price;
      }
      return result;
    },
    staleTime: 60_000,
    refetchInterval: 60_000
  });

  /** Convert an amount in a given currency to USD. Returns null if no rate available. */
  function toUsd(amount: string | undefined, currency: string): number | null {
    if (amount == null || currency === "USD") return null;
    const num = parseFloat(amount);
    if (isNaN(num)) return null;
    const rate = rates[currency];
    if (rate == null) return null;
    return num * rate;
  }

  return { rates, toUsd };
}

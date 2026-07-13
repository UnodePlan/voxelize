import {
  Backpack,
  Box,
  Check,
  CircleAlert,
  Clock3,
  Coins,
  createIcons,
  Diamond,
  Gem,
  Heart,
  LogOut,
  Pickaxe,
  RefreshCw,
  ShieldCheck,
  Swords,
  Timer,
  UserRound,
  Wallet,
  Warehouse,
  X,
} from "lucide";

const PRODUCT_ICONS = {
  Backpack,
  Box,
  Check,
  CircleAlert,
  Clock3,
  Coins,
  Diamond,
  Gem,
  Heart,
  LogOut,
  Pickaxe,
  RefreshCw,
  ShieldCheck,
  Swords,
  Timer,
  UserRound,
  Wallet,
  Warehouse,
  X,
};

export function hydrateIcons(root: Element): void {
  createIcons({
    icons: PRODUCT_ICONS,
    root,
    attrs: {
      width: 18,
      height: 18,
      "stroke-width": 1.8,
      "aria-hidden": "true",
    },
  });
}

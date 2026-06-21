import QRCode from "react-qr-code";

import { Logo } from "@/application/layout/logo";

// A branded deposit-address QR: high error-correction modules in deep navy on a white
// rounded plate, with the EV mark inset in the centre (level H tolerates the occlusion).
// `value` is the on-chain address string the wallet renders alongside it.
export function DepositQr({ value }: { value: string }) {
  return (
    <div className="relative flex size-44 shrink-0 items-center justify-center rounded-2xl bg-white p-3.5 ring-1 ring-border">
      <QRCode value={value} level="H" size={256} fgColor="#0c1626" bgColor="#ffffff" className="h-full w-full" />
      <span className="absolute flex size-10 items-center justify-center rounded-xl bg-white shadow-[0_0_0_4px_white]">
        <Logo className="h-5 w-auto text-main-accent-t1" />
      </span>
    </div>
  );
}

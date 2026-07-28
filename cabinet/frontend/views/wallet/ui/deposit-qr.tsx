import QRCode from "react-qr-code";

import { Logo } from "@/application/layout/logo";

// A branded deposit-address QR (Figma `qr`): high error-correction modules in deep navy on a
// white rounded plate, with the EV mark inset in the centre (level H tolerates the occlusion).
// 160px on mobile, 180px on desktop — matching the two frames. `value` is the on-chain address
// string the wallet renders alongside it.
export function DepositQr({ value }: { value: string }) {
  return (
    <div className="relative flex size-40 shrink-0 items-center justify-center rounded-xl border border-border bg-white p-2.5 lg:size-[180px] lg:rounded-[18px] lg:p-3.5">
      <QRCode value={value} level="H" size={256} fgColor="#0c1626" bgColor="#ffffff" className="h-full w-full" />
      <span className="absolute flex size-7 items-center justify-center rounded-lg bg-white shadow-[0_0_0_3px_white]">
        <Logo className="h-3.5 w-auto text-main-accent-t1" />
      </span>
    </div>
  );
}

import Image from "next/image";
import { Text } from "@/shared/ui/text";
import { ASSETS } from "@/shared/config/assets";

/** Server Component — framed coastal image with floating key-stats card. */
export function BoardroomImage() {
  return (
    <div className="relative">
      <div className="relative aspect-[4/5] rounded-xl overflow-hidden border border-main-mist/10 shadow-2xl">
        <Image
          src={ASSETS.quynhon_future}
          alt="Quy Nhon coastal development"
          fill
          sizes="(min-width: 1024px) 45vw, 100vw"
          className="object-cover opacity-80"
          priority
        />
        <div className="absolute inset-0 bg-gradient-to-t from-main-black via-main-black/20 to-transparent" />
      </div>

      <div className="absolute -bottom-6 -left-6 sm:left-6 bg-main-black/85 border border-main-mist/10 rounded-xl px-6 py-5 backdrop-blur-sm shadow-2xl grid grid-cols-2 gap-6">
        <div>
          <Text variant="secondary" className="text-[10px] font-mono-tech uppercase tracking-widest mb-1">
            AUM Under Advisory
          </Text>
          <p className="text-xl sm:text-2xl font-serif-display text-white font-bold">
            $145M
          </p>
        </div>
        <div>
          <Text variant="secondary" className="text-[10px] font-mono-tech uppercase tracking-widest mb-1">
            Coastline
          </Text>
          <p className="text-xl sm:text-2xl font-serif-display text-main-accent-t1 font-bold">
            72 km
          </p>
        </div>
      </div>
    </div>
  );
}

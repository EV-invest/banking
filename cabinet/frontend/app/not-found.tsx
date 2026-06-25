import Link from "next/link";

export default function NotFound() {
  return (
    <div className="container flex min-h-[60vh] flex-col items-center justify-center gap-4 text-center">
      <h1 className="font-serif text-4xl">404</h1>
      <p className="text-muted-foreground">This page isn&apos;t here.</p>
      <Link href="/" className="text-main-accent-t1 hover:underline">
        Back home
      </Link>
    </div>
  );
}

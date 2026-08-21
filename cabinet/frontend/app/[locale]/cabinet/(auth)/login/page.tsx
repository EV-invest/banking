import { LoginView } from "@/views/login/ui/login";

export default function Page({ searchParams }: { searchParams: Promise<{ error?: string; returnTo?: string }> }) {
  return <LoginView searchParams={searchParams} />;
}

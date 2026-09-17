import { LoginView, type LoginSearchParams } from "@/views/login/ui/login";

export default function Page({ searchParams }: { searchParams: Promise<LoginSearchParams> }) {
  return <LoginView searchParams={searchParams} />;
}

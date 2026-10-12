import type { UseQueryResult } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { Refusal } from "#/components/refusal";
import { Skeleton } from "#/components/ui/skeleton";

export function WorkRead<Data>({
	read,
	loading,
	children,
}: {
	read: UseQueryResult<Data>;
	loading?: ReactNode;
	children: (data: Data) => ReactNode;
}) {
	if (read.isPending) return loading ?? <Skeleton className="h-8 w-full" />;
	if (read.isError) return <Refusal error={read.error} retry={() => void read.refetch()} />;
	return children(read.data);
}

import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { backend } from "../services/backend";
import type { DownloadJob } from "../types/domain";

export function useDownloads() {
  const [jobs, setJobs] = useState<DownloadJob[]>([]);

  useEffect(() => {
    let disposed = false;
    void backend.listDownloads().then((items) => { if (!disposed) setJobs(items); }).catch(() => undefined);
    const unlisten = listen<DownloadJob>("download://changed", ({ payload }) => {
      setJobs((current) => {
        const remaining = current.filter((job) => job.id !== payload.id);
        return [payload, ...remaining].sort((left, right) => right.createdAt.localeCompare(left.createdAt));
      });
    });
    return () => {
      disposed = true;
      void unlisten.then((stop) => stop());
    };
  }, []);

  const activeCount = useMemo(
    () => jobs.filter((job) => !["completed", "failed", "cancelled"].includes(job.status)).length,
    [jobs],
  );

  return {
    jobs,
    activeCount,
    add: (job: DownloadJob) => setJobs((current) => [job, ...current.filter((item) => item.id !== job.id)]),
    cancel: (id: string) => backend.cancelDownload(id),
  };
}


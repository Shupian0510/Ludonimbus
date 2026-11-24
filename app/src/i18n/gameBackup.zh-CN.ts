// 游戏本体备份和云盘测速的界面文字（zh-CN.ts 里以 gameBackup 导出）。

export const gameBackup = {
  speed: {
    title: "云盘测速",
    intro: "向当前云盘上传、再下载一批随机测试文件（单线程和并行各一次），测完自动删除。用来判断这个云盘适不适合备份游戏本体。",
    amount: "测试数据量",
    threads: "并行数",
    start: "开始测速",
    cancel: "停止",
    noCloud: "先连接云盘再测速。",
    phase: {
      latency: "测量请求延迟…",
      uploadSerial: "上传（单线程）",
      uploadParallel: "上传（并行）",
      downloadSerial: "下载（单线程）",
      downloadParallel: "下载（并行）",
      cleanup: "删除测试文件…",
    } as Record<string, string>,
    upload: "上传",
    download: "下载",
    serial: "单线程",
    parallel: (n: number) => `${n} 线程`,
    capped: "到时间上限提前结束",
    request: (w: number, r: number) => `每次请求的固定开销：写入约 ${w} ms，读取约 ${r} ms`,
    estimate: (size: string, up: string, down: string) => `${size} 的游戏：上传约 ${up}，下载约 ${down}`,
    verdict: {
      good: "适合备份游戏本体",
      slow: "可以用，但大游戏要几个小时",
      tooSlow: "太慢，不建议备份几 GB 以上的游戏",
      failed: "测速失败",
    } as Record<string, string>,
    cleanupFailed: (e: string) => `有测试文件没删掉：${e}`,
    failed: (e: string) => `测速失败：${e}`,
  },
  duration: {
    minutes: (n: number) => `${n} 分钟`,
    hours: (n: number) => `${n} 小时`,
    days: (n: number) => `${n} 天`,
    unknown: "无法估算",
  },
};

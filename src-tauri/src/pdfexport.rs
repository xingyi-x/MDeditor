//! PDF 导出：绕过系统打印对话框，直接调用 WebView2 的 PrintToPdf。
//!
//! 为什么不用 `window.print()`：
//! WebView2 的 `print()` 走 Chromium 打印预览，预览里"页眉和页脚"默认开启，
//! 会往成品 PDF 里塞上「日期 + 文档标题」（页眉）与「tauri.localhost + 页码」（页脚），
//! 而且这个开关只能由用户在预览界面手动取消，代码无法预设。
//!
//! `ICoreWebView2_16`/`_7` 的 `PrintToPdf` 配合 `ICoreWebView2PrintSettings`
//! 可以把 `ShouldPrintHeaderAndFooter` 明确置为 false，并获得确定的纸张尺寸与页边距。

use serde::Deserialize;

/// 打印参数（英寸）。前端按纸张规格 mm → inch 换算后传入。
#[derive(Deserialize, Clone, Copy, Debug)]
pub struct PdfOptions {
    pub width_in: f64,
    pub height_in: f64,
    #[serde(default)]
    pub margin_top_in: f64,
    #[serde(default)]
    pub margin_right_in: f64,
    #[serde(default)]
    pub margin_bottom_in: f64,
    #[serde(default)]
    pub margin_left_in: f64,
}

#[cfg(windows)]
mod imp {
    use super::PdfOptions;
    use std::sync::mpsc;
    use std::time::Duration;
    use tauri::WebviewWindow;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_2, ICoreWebView2_7, ICoreWebView2Environment6, ICoreWebView2PrintSettings,
    };
    use webview2_com::PrintToPdfCompletedHandler;
    use windows::core::{Interface, HSTRING};

    /// 主线程里启动 PrintToPdf，结果通过 channel 回传
    unsafe fn start(
        controller: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller,
        path: String,
        opt: PdfOptions,
        tx: mpsc::Sender<Result<(), String>>,
    ) -> Result<(), String> {
        let core = controller.CoreWebView2().map_err(|e| e.to_string())?;
        let core2: ICoreWebView2_2 = core.cast().map_err(|e| e.to_string())?;
        let env = core2.Environment().map_err(|e| e.to_string())?;
        let env6: ICoreWebView2Environment6 = env.cast().map_err(|e| e.to_string())?;
        let settings: ICoreWebView2PrintSettings =
            env6.CreatePrintSettings().map_err(|e| e.to_string())?;

        // 关键：关掉浏览器的页眉 / 页脚
        settings
            .SetShouldPrintHeaderAndFooter(false)
            .map_err(|e| e.to_string())?;
        // 允许打印背景图形，但 print.css 会把正文 / 代码块 / 引用 / 表头强制成
        // 白底黑字（打印友好），所以最终 PDF 不保留彩色主题配色。
        settings
            .SetShouldPrintBackgrounds(true)
            .map_err(|e| e.to_string())?;
        settings.SetScaleFactor(1.0).map_err(|e| e.to_string())?;
        settings
            .SetPageWidth(opt.width_in)
            .map_err(|e| e.to_string())?;
        settings
            .SetPageHeight(opt.height_in)
            .map_err(|e| e.to_string())?;
        settings
            .SetMarginTop(opt.margin_top_in)
            .map_err(|e| e.to_string())?;
        settings
            .SetMarginRight(opt.margin_right_in)
            .map_err(|e| e.to_string())?;
        settings
            .SetMarginBottom(opt.margin_bottom_in)
            .map_err(|e| e.to_string())?;
        settings
            .SetMarginLeft(opt.margin_left_in)
            .map_err(|e| e.to_string())?;

        let core7: ICoreWebView2_7 = core.cast().map_err(|e| e.to_string())?;
        let handler = PrintToPdfCompletedHandler::create(Box::new(move |hr, ok| {
            let _ = if hr.is_ok() && ok {
                tx.send(Ok(()))
            } else {
                // hr: Result<(), windows::core::Error>；ok: 是否成功写出 PDF
                tx.send(Err(format!(
                    "PrintToPdf 失败（success={}，err={:?}）",
                    ok,
                    hr.err().map(|e| e.to_string())
                )))
            };
            Ok(())
        }));
        core7
            .PrintToPdf(&HSTRING::from(path.as_str()), &settings, &handler)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn export(
        window: &WebviewWindow,
        path: String,
        opt: PdfOptions,
    ) -> Result<(), String> {
        let (tx, rx) = mpsc::channel::<Result<(), String>>();
        let tx2 = tx.clone();
        window
            .with_webview(move |pw| {
                // with_webview 在主线程执行；这里只负责启动，不能阻塞等待
                let spawned = unsafe { start(pw.controller(), path.clone(), opt, tx.clone()) };
                if let Err(e) = spawned {
                    let _ = tx.send(Err(e));
                }
            })
            .map_err(|e| e.to_string())?;
        drop(tx2);

        // 回调没执行（channel 断开）和真正超时是两回事，别一律报「超时」
        match tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(180)))
            .await
        {
            Err(e) => Err(format!("PDF 导出任务异常终止：{}", e)),
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(e))) => Err(e),
            Ok(Err(mpsc::RecvTimeoutError::Disconnected)) => {
                Err("PDF 导出回调未执行（WebView 可能已关闭）".into())
            }
            Ok(Err(mpsc::RecvTimeoutError::Timeout)) => {
                Err("PDF 导出超时（180 秒未完成）".into())
            }
        }
    }
}

#[cfg(windows)]
pub use imp::export;

#[cfg(not(windows))]
pub async fn export(
    _window: &tauri::WebviewWindow,
    _path: String,
    _opt: PdfOptions,
) -> Result<(), String> {
    Err("PDF 导出仅支持 Windows".into())
}

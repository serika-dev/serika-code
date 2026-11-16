#include "windowchrome.h"

#include <QEvent>
#include <QGuiApplication>
#include <QTimer>
#include <QWindow>

#ifdef Q_OS_WIN
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <dwmapi.h>

// TitleBar.qml draws three 46x36 buttons, right-aligned, flush with the top.
constexpr int kCaptionButtonWidth = 46;
constexpr int kCaptionButtonHeight = 36;
constexpr int kCaptionButtonCount = 3;

#ifndef DWMWA_WINDOW_CORNER_PREFERENCE
#define DWMWA_WINDOW_CORNER_PREFERENCE 33
#endif
#ifndef DWMWCP_ROUND
#define DWMWCP_ROUND 2
#endif

int resizeBorder(HWND hwnd) {
    const auto dpiForWindow = reinterpret_cast<UINT(WINAPI *)(HWND)>(
        GetProcAddress(GetModuleHandleW(L"user32.dll"), "GetDpiForWindow"));
    const auto metricsForDpi = reinterpret_cast<int(WINAPI *)(int, UINT)>(
        GetProcAddress(GetModuleHandleW(L"user32.dll"), "GetSystemMetricsForDpi"));
    if (dpiForWindow && metricsForDpi) {
        const UINT dpi = dpiForWindow(hwnd);
        return metricsForDpi(SM_CXFRAME, dpi) + metricsForDpi(SM_CXPADDEDBORDER, dpi);
    }
    return GetSystemMetrics(SM_CXFRAME) + GetSystemMetrics(SM_CXPADDEDBORDER);
}

// Which caption button the physical screen point lands on: "min", "max", "close", or empty.
QString captionButton(QWindow *window, HWND hwnd, int physX, int physY) {
    if (!window) return {};
    const qreal dpr = window->devicePixelRatio();
    if (dpr <= 0) return {};
    POINT origin{0, 0};
    ClientToScreen(hwnd, &origin);
    const int x = qRound((physX - origin.x) / dpr);
    const int y = qRound((physY - origin.y) / dpr);
    if (y < 0 || y >= kCaptionButtonHeight) return {};
    const int span = kCaptionButtonWidth * kCaptionButtonCount;
    const int left = window->width() - span;
    if (x < left || x >= window->width()) return {};
    switch ((x - left) / kCaptionButtonWidth) {
    case 0: return QStringLiteral("min");
    case 1: return QStringLiteral("max");
    default: return QStringLiteral("close");
    }
}

LRESULT captionHit(const QString &button) {
    if (button == QLatin1String("min")) return HTMINBUTTON;
    if (button == QLatin1String("max")) return HTMAXBUTTON;
    if (button == QLatin1String("close")) return HTCLOSE;
    return HTCLIENT;
}
#endif

WindowChrome::WindowChrome(QObject *parent) : QObject(parent) {}

WindowChrome::~WindowChrome() {
    if (qApp) qApp->removeNativeEventFilter(this);
}

bool WindowChrome::active() const {
#ifdef Q_OS_WIN
    return true;
#else
    return false;
#endif
}

void WindowChrome::attach(QWindow *window) {
    if (m_window) m_window->removeEventFilter(this);
    m_window = window;
    if (!window) return;
    window->installEventFilter(this);
    if (qApp) qApp->installNativeEventFilter(this);
    scheduleApply();
}

bool WindowChrome::eventFilter(QObject *watched, QEvent *event) {
    if (watched == m_window
        && (event->type() == QEvent::Show || event->type() == QEvent::WinIdChange
            || event->type() == QEvent::WindowStateChange))
        scheduleApply();
    return false;
}

void WindowChrome::scheduleApply() {
#ifdef Q_OS_WIN
    if (m_applyPending) return;
    m_applyPending = true;
    QTimer::singleShot(0, this, [this] {
        m_applyPending = false;
        applyFrame();
    });
#else
    Q_UNUSED(this);
#endif
}

void WindowChrome::applyFrame() {
#ifdef Q_OS_WIN
    if (!m_window || m_applying) return;
    m_applying = true;
    // Cache the handle. Calling winId() from the native event filter re-enters
    // window creation and CreateWindowEx fails in a loop.
    const HWND hwnd = reinterpret_cast<HWND>(m_window->winId());
    m_hwnd = hwnd;
    if (!hwnd) {
        m_applying = false;
        return;
    }

    // WS_THICKFRAME and WS_CAPTION are what make Win+arrow, drag-to-edge snap
    // and the minimize animation work. WM_NCCALCSIZE drops the pixels of that
    // frame so the QML title bar stays the only one drawn.
    const LONG_PTR style = GetWindowLongPtrW(hwnd, GWL_STYLE);
    const LONG_PTR want = style | WS_THICKFRAME | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
    bool refresh = want != style;
    if (refresh) SetWindowLongPtrW(hwnd, GWL_STYLE, want);

    if (!m_frameReady) {
        const MARGINS margins{-1, -1, -1, -1};
        DwmExtendFrameIntoClientArea(hwnd, &margins);
        const int corner = DWMWCP_ROUND;
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner, sizeof(corner));
        m_frameReady = true;
        refresh = true;
    }
    // Skip the frame update once the style is already in place. SetWindowPos
    // can come back as another state change, and applying it again would loop.
    if (refresh) {
        SetWindowPos(hwnd, nullptr, 0, 0, 0, 0,
                     SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
    m_applying = false;
#endif
}

void WindowChrome::setHot(const QString &hot) {
    if (m_hot == hot) return;
    m_hot = hot;
    emit hotChanged();
}

bool WindowChrome::nativeEventFilter(const QByteArray &eventType, void *message, qintptr *result) {
#ifdef Q_OS_WIN
    if (!m_window || !m_hwnd || !result) return false;
    if (eventType != "windows_generic_MSG" && eventType != "windows_dispatcher_MSG") return false;

    const auto *msg = static_cast<const MSG *>(message);
    const HWND hwnd = static_cast<HWND>(m_hwnd);
    if (msg->hwnd != hwnd) return false;

    switch (msg->message) {
    case WM_NCCALCSIZE:
        // Let Qt finish CreateWindowEx. Eating this message before the frame
        // is applied makes creation fail and retry forever.
        if (!m_frameReady || !msg->wParam || !IsWindowVisible(hwnd)) return false;
        // A maximized window is positioned so its frame hangs off the work area.
        // Inset by that frame or the chat draws under the taskbar and off-screen.
        if (IsZoomed(hwnd)) {
            auto *params = reinterpret_cast<NCCALCSIZE_PARAMS *>(msg->lParam);
            const int border = resizeBorder(hwnd);
            params->rgrc[0].top += border;
            params->rgrc[0].bottom -= border;
            params->rgrc[0].left += border;
            params->rgrc[0].right -= border;
        }
        *result = 0;
        return true;

    case WM_NCHITTEST: {
        const int x = static_cast<int>(static_cast<short>(LOWORD(msg->lParam)));
        const int y = static_cast<int>(static_cast<short>(HIWORD(msg->lParam)));
        if (!IsZoomed(hwnd)) {
            RECT rect{};
            GetWindowRect(hwnd, &rect);
            const int border = resizeBorder(hwnd);
            const bool left = x < rect.left + border;
            const bool right = x >= rect.right - border;
            const bool top = y < rect.top + border;
            const bool bottom = y >= rect.bottom - border;
            if (left || right || top || bottom) {
                setHot({});
                if (top && left) *result = HTTOPLEFT;
                else if (top && right) *result = HTTOPRIGHT;
                else if (bottom && left) *result = HTBOTTOMLEFT;
                else if (bottom && right) *result = HTBOTTOMRIGHT;
                else if (left) *result = HTLEFT;
                else if (right) *result = HTRIGHT;
                else if (top) *result = HTTOP;
                else *result = HTBOTTOM;
                return true;
            }
        }
        const QString button = captionButton(m_window, hwnd, x, y);
        if (!button.isEmpty()) {
            setHot(button);
            // HTMAXBUTTON is what makes Windows 11 open snap layouts on hover.
            *result = captionHit(button);
            return true;
        }
        setHot({});
        return false;
    }

    case WM_NCMOUSELEAVE:
    case WM_MOUSELEAVE:
        setHot({});
        return false;

    case WM_NCLBUTTONDOWN:
        // The snap-layout flyout is a separate window, so a click on a layout
        // never arrives here. A click on the button itself does.
        if (msg->wParam == HTMINBUTTON) ShowWindow(hwnd, SW_MINIMIZE);
        else if (msg->wParam == HTMAXBUTTON) ShowWindow(hwnd, IsZoomed(hwnd) ? SW_RESTORE : SW_MAXIMIZE);
        else if (msg->wParam == HTCLOSE) PostMessageW(hwnd, WM_CLOSE, 0, 0);
        else return false;
        *result = 0;
        return true;

    case WM_NCLBUTTONUP:
        if (msg->wParam == HTMINBUTTON || msg->wParam == HTMAXBUTTON || msg->wParam == HTCLOSE) {
            *result = 0;
            return true;
        }
        return false;

    default:
        return false;
    }
#else
    Q_UNUSED(eventType);
    Q_UNUSED(message);
    Q_UNUSED(result);
    return false;
#endif
}

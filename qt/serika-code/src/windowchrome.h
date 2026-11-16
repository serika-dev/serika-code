#pragma once

#include <QAbstractNativeEventFilter>
#include <QObject>
#include <QPointer>

class QWindow;

// Keeps a frameless window participating in the platform's window manager.
// On Windows that means Aero Snap, Win+arrow, snap layouts on the maximize
// button, resize from the edges, and a drop shadow, without a stock title bar.
class WindowChrome : public QObject, public QAbstractNativeEventFilter {
    Q_OBJECT
    Q_PROPERTY(bool active READ active CONSTANT)
    Q_PROPERTY(QString hot READ hot NOTIFY hotChanged)

public:
    explicit WindowChrome(QObject *parent = nullptr);
    ~WindowChrome() override;

    bool active() const;
    QString hot() const { return m_hot; }

    void attach(QWindow *window);
    bool nativeEventFilter(const QByteArray &eventType, void *message, qintptr *result) override;

signals:
    void hotChanged();

protected:
    bool eventFilter(QObject *watched, QEvent *event) override;

private:
    void scheduleApply();
    void applyFrame();
    void setHot(const QString &hot);

    QPointer<QWindow> m_window;
    QString m_hot;
    bool m_applyPending = false;
    bool m_applying = false;
    bool m_frameReady = false;
#ifdef Q_OS_WIN
    void *m_hwnd = nullptr;
#endif
};

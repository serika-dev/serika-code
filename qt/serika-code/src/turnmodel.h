#pragma once

#include <QAbstractListModel>
#include <QList>
#include <QString>
#include <QTimer>
#include <QVariantList>

struct Turn {
    QString role;
    QString raw;       // exactly what the model produced, used while streaming
    QString content;   // raw text with tool-call fences removed
    QString reasoning;
    QVariantList tools;
    QVariantList attachments; // files and images sent with a user message
    QString model;            // model that wrote an assistant reply
    QString switchFrom;       // role "switch": the chat moved from this model…
    QString switchTo;         // …to this one
    bool streaming = false;
    int shown = -1;           // while a reply is typed out: how much of content is on screen (-1: all)
};

/// Chat turns for the thread view. Row-level updates keep the list from resetting
/// (and jumping) while tokens stream in.
class TurnModel : public QAbstractListModel {
    Q_OBJECT
    Q_PROPERTY(int count READ count NOTIFY countChanged)

public:
    enum Role { TurnRole = Qt::UserRole + 1, ContentRole, ReasoningRole, ToolsRole, StreamingRole, AttachmentsRole, ModelRole, SwitchFromRole, SwitchToRole };

    explicit TurnModel(QObject *parent = nullptr);

    int count() const { return int(m_turns.size()); }
    int rowCount(const QModelIndex &parent = QModelIndex()) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    void replaceAll(const QVariantList &turns);
    void clear();
    int append(const Turn &turn);
    void insert(int row, const Turn &turn);
    Turn *last();
    Turn *lastAssistant(int *row = nullptr);
    void changed(int row, const QList<int> &roles);
    // Streamed text arrived for a row: type it out at an even pace instead of in bursts.
    // shownBefore is how much of the content was on screen before the new text.
    void reveal(int row, int shownBefore);
    const QList<Turn> &turns() const { return m_turns; }

    static QString stripToolFences(const QString &raw);

signals:
    void countChanged();

private:
    void pace();

    QList<Turn> m_turns;
    QTimer m_pace;
};

#pragma once

#include <QAbstractListModel>
#include <QList>
#include <QString>
#include <QVariantList>

struct Turn {
    QString role;
    QString raw;       // exactly what the model produced, used while streaming
    QString content;   // raw text with tool-call fences removed
    QString reasoning;
    QVariantList tools;
    QVariantList attachments; // files and images sent with a user message
    bool streaming = false;
};

/// Chat turns for the thread view. Row-level updates keep the list from resetting
/// (and jumping) while tokens stream in.
class TurnModel : public QAbstractListModel {
    Q_OBJECT
    Q_PROPERTY(int count READ count NOTIFY countChanged)

public:
    enum Role { TurnRole = Qt::UserRole + 1, ContentRole, ReasoningRole, ToolsRole, StreamingRole, AttachmentsRole };

    explicit TurnModel(QObject *parent = nullptr);

    int count() const { return int(m_turns.size()); }
    int rowCount(const QModelIndex &parent = QModelIndex()) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    void replaceAll(const QVariantList &turns);
    void clear();
    int append(const Turn &turn);
    Turn *last();
    Turn *lastAssistant(int *row = nullptr);
    void changed(int row, const QList<int> &roles);
    const QList<Turn> &turns() const { return m_turns; }

    static QString stripToolFences(const QString &raw);

signals:
    void countChanged();

private:
    QList<Turn> m_turns;
};

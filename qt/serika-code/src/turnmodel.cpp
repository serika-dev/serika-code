#include "turnmodel.h"

#include <QRegularExpression>
#include <QVariantMap>

TurnModel::TurnModel(QObject *parent) : QAbstractListModel(parent) {}

int TurnModel::rowCount(const QModelIndex &parent) const {
    return parent.isValid() ? 0 : int(m_turns.size());
}

QVariant TurnModel::data(const QModelIndex &index, int role) const {
    if (!index.isValid() || index.row() < 0 || index.row() >= m_turns.size())
        return {};
    const Turn &turn = m_turns.at(index.row());
    switch (role) {
    case TurnRole:
        return turn.role;
    case ContentRole:
        return turn.content;
    case ReasoningRole:
        return turn.reasoning;
    case ToolsRole:
        return turn.tools;
    case StreamingRole:
        return turn.streaming;
    case AttachmentsRole:
        return turn.attachments;
    default:
        return {};
    }
}

QHash<int, QByteArray> TurnModel::roleNames() const {
    return {
        {TurnRole, "turn"},
        {ContentRole, "content"},
        {ReasoningRole, "reasoning"},
        {ToolsRole, "tools"},
        {StreamingRole, "streaming"},
        {AttachmentsRole, "attachments"},
    };
}

void TurnModel::replaceAll(const QVariantList &turns) {
    beginResetModel();
    m_turns.clear();
    for (const QVariant &item : turns) {
        const QVariantMap map = item.toMap();
        Turn turn;
        turn.role = map.value(QStringLiteral("role")).toString();
        turn.raw = map.value(QStringLiteral("content")).toString();
        turn.content = turn.raw;
        turn.reasoning = map.value(QStringLiteral("reasoning")).toString();
        turn.tools = map.value(QStringLiteral("tools")).toList();
        turn.attachments = map.value(QStringLiteral("attachments")).toList();
        m_turns.append(turn);
    }
    endResetModel();
    emit countChanged();
}

void TurnModel::clear() {
    if (m_turns.isEmpty())
        return;
    beginResetModel();
    m_turns.clear();
    endResetModel();
    emit countChanged();
}

int TurnModel::append(const Turn &turn) {
    const int row = int(m_turns.size());
    beginInsertRows(QModelIndex(), row, row);
    m_turns.append(turn);
    endInsertRows();
    emit countChanged();
    return row;
}

Turn *TurnModel::last() { return m_turns.isEmpty() ? nullptr : &m_turns.last(); }

Turn *TurnModel::lastAssistant(int *row) {
    for (int i = int(m_turns.size()) - 1; i >= 0; --i) {
        if (m_turns.at(i).role == QLatin1String("assistant")) {
            if (row)
                *row = i;
            return &m_turns[i];
        }
    }
    return nullptr;
}

void TurnModel::changed(int row, const QList<int> &roles) {
    const QModelIndex idx = index(row);
    emit dataChanged(idx, idx, roles);
}

QString TurnModel::stripToolFences(const QString &raw) {
    static const QRegularExpression closed(
        QStringLiteral("```(?:serika[-_]tool|tool_call|tool)[ \\t]*\\n.*?```\\n?"),
        QRegularExpression::DotMatchesEverythingOption);
    static const QRegularExpression open(
        QStringLiteral("```(?:serika[-_]tool|tool_call|tool)[ \\t]*(?:\\n.*)?$"),
        QRegularExpression::DotMatchesEverythingOption);
    QString out = raw;
    out.remove(closed);
    out.remove(open);
    return out;
}

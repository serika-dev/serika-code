#include "turnmodel.h"

#include <QRegularExpression>
#include <QVariantMap>

TurnModel::TurnModel(QObject *parent) : QAbstractListModel(parent) {
    m_pace.setInterval(16);
    m_pace.setTimerType(Qt::PreciseTimer);
    connect(&m_pace, &QTimer::timeout, this, &TurnModel::pace);
}

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
        return turn.shown >= 0 ? turn.content.left(turn.shown) : turn.content;
    case ReasoningRole:
        return turn.reasoning;
    case ToolsRole:
        return turn.tools;
    case StreamingRole:
        // Still typing out counts as streaming, so the footer waits for the last word.
        return turn.streaming || turn.shown >= 0;
    case AttachmentsRole:
        return turn.attachments;
    case ModelRole:
        return turn.model;
    case SwitchFromRole:
        return turn.switchFrom;
    case SwitchToRole:
        return turn.switchTo;
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
        {ModelRole, "replyModel"},
        {SwitchFromRole, "switchFrom"},
        {SwitchToRole, "switchTo"},
    };
}

void TurnModel::replaceAll(const QVariantList &turns) {
    m_pace.stop();
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
        turn.model = map.value(QStringLiteral("model")).toString();
        turn.switchFrom = map.value(QStringLiteral("from")).toString();
        turn.switchTo = map.value(QStringLiteral("to")).toString();
        m_turns.append(turn);
    }
    endResetModel();
    emit countChanged();
}

void TurnModel::clear() {
    m_pace.stop();
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

void TurnModel::insert(int row, const Turn &turn) {
    row = qBound(0, row, int(m_turns.size()));
    beginInsertRows(QModelIndex(), row, row);
    m_turns.insert(row, turn);
    endInsertRows();
    emit countChanged();
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

void TurnModel::reveal(int row, int shownBefore) {
    if (row < 0 || row >= m_turns.size())
        return;
    Turn &turn = m_turns[row];
    if (turn.shown < 0)
        turn.shown = qBound(0, shownBefore, int(turn.content.size()));
    if (!m_pace.isActive())
        m_pace.start();
}

// One frame of typing. The pace follows the backlog: a trickle of tokens shows at reading
// speed a few hundred milliseconds behind, and a big block (a whole message at once, or a
// burst after a pause) types out over a second or two instead of landing in one go.
// Words appear whole.
void TurnModel::pace() {
    bool busy = false;
    for (int row = 0; row < m_turns.size(); ++row) {
        Turn &turn = m_turns[row];
        if (turn.shown < 0)
            continue;
        const int total = int(turn.content.size());
        const int backlog = total - turn.shown;
        if (backlog <= 0) {
            turn.shown = -1;
            changed(row, {ContentRole, StreamingRole});
            continue;
        }
        int next = turn.shown + qBound(2, (backlog + 23) / 24, 60);
        // Finish the word in progress, if it ends soon.
        for (int i = next, end = qMin(total, next + 12); i < end; ++i) {
            if (turn.content.at(i).isSpace()) {
                next = i;
                break;
            }
        }
        next = qMin(next, total);
        if (next < total && turn.content.at(next).isLowSurrogate())
            ++next;
        turn.shown = next >= total ? -1 : next;
        if (turn.shown < 0)
            changed(row, {ContentRole, StreamingRole});
        else
            changed(row, {ContentRole});
        busy = busy || turn.shown >= 0;
    }
    if (!busy)
        m_pace.stop();
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

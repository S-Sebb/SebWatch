#include "version.h"

#include <QTest>

// Proves the core library links into Qt Test executables; real tests arrive with the core.
class TestVersion : public QObject
{
    Q_OBJECT

private slots:
    void matchesProjectVersion() { QCOMPARE(sebwatch::version(), QStringLiteral(SEBWATCH_EXPECTED_VERSION)); }
};

QTEST_GUILESS_MAIN(TestVersion)
#include "tst_version.moc"

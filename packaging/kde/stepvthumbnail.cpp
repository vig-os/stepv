// KF6 thumbnail plugin for Dolphin and every KIO-based file manager.
//
// Deliberately thin: it shells out to the same `stepv` CLI the GNOME
// .thumbnailer uses, so there is one product boundary (plan.md §4) and the
// kernel's crash containment, timeout and memory cap apply here unchanged.
// A crashing CAD file can never take Dolphin's thumbnail process with it.

#include <KIO/ThumbnailCreator>
#include <KPluginFactory>

#include <QImage>
#include <QProcess>
#include <QStandardPaths>
#include <QTemporaryDir>

class StepvThumbnail : public KIO::ThumbnailCreator {
    Q_OBJECT
public:
    StepvThumbnail(QObject* parent, const QVariantList& args) : KIO::ThumbnailCreator(parent, args) {}

    KIO::ThumbnailResult create(const KIO::ThumbnailRequest& request) override {
        const QString path = request.url().toLocalFile();
        if (path.isEmpty()) return KIO::ThumbnailResult::fail();

        QString stepv = QStandardPaths::findExecutable(QStringLiteral("stepv"));
        if (stepv.isEmpty()) return KIO::ThumbnailResult::fail();

        QTemporaryDir dir;
        if (!dir.isValid()) return KIO::ThumbnailResult::fail();
        const QString out = dir.filePath(QStringLiteral("thumb.png"));
        const int size = qMax(request.targetSize().width(), request.targetSize().height());

        QProcess p;
        // --no-cache: KIO keeps its own thumbnail cache.
        p.start(stepv, {path, QStringLiteral("--png"), out, QStringLiteral("--size"),
                        QString::number(qBound(16, size, 4096)), QStringLiteral("--timeout"),
                        QStringLiteral("15"), QStringLiteral("--no-cache")});
        // stepv enforces its own 15 s cap; this is only a backstop.
        if (!p.waitForFinished(20000)) {
            p.kill();
            p.waitForFinished();
            return KIO::ThumbnailResult::fail();
        }
        if (p.exitStatus() != QProcess::NormalExit || p.exitCode() != 0)
            return KIO::ThumbnailResult::fail();

        QImage img(out);
        return img.isNull() ? KIO::ThumbnailResult::fail() : KIO::ThumbnailResult::pass(img);
    }
};

K_PLUGIN_CLASS_WITH_JSON(StepvThumbnail, "stepvthumbnail.json")

#include "stepvthumbnail.moc"
